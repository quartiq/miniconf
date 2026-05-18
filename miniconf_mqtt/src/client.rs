mod request;
mod sync;

use core::marker::PhantomData;

use heapless::Deque;
use miniconf::{
    ConstPath, DescendError, Indices, IntoKeys, ResolveError, Schema, SerdeError,
    TreeDeserializeOwned, TreeSchema, TreeSerialize, ValueError, json_core,
};
use minimq::{
    ConfigBuilder, ConfigError, ConnectEvent, Error as MqttError, InboundPublish, Io, Op, OpStatus,
    OwnedResponseTarget, Property, PubError, Publication, QoS, ResourceError, Session, TopicString,
    Will, publication::ToPayload, types::Utf8String,
};
use serde::Serialize;
use serde_json_core::ser::Error as JsonSerError;

use crate::{
    EncodeError, MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH,
    message::{DepthError, simple_pub_error},
    schema::{SchemaDefs, SchemaSync, SettingsSync, serialize_schema_page},
};
use request::FollowUp;

/// Exact leaf indices produced by MM2 request handling.
pub type ChangedKey = Indices<[usize; crate::MAX_DEPTH]>;

#[derive(Debug, PartialEq, thiserror::Error)]
/// MM2 setup, tree, or MQTT session error.
pub enum Error<E> {
    /// Tree traversal or path resolution failed before any MQTT I/O.
    #[error("tree path resolution failed: {0}")]
    Tree(DescendError<()>),
    /// MQTT session or publication failure.
    #[error(transparent)]
    Mqtt(#[from] MqttError<E>),
}

impl<E> From<DescendError<()>> for Error<E> {
    fn from(value: DescendError<()>) -> Self {
        Self::Tree(value)
    }
}

#[derive(Default)]
pub(crate) struct Manifest {
    pub(crate) epoch: u32,
    pub(crate) schema_rev: u32,
    pub(crate) schema_pages: usize,
}

#[derive(Debug)]
pub(crate) enum PayloadError {
    Json(JsonSerError),
    Schema(usize),
    Leaf(DepthError<JsonSerError>),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PendingOp {
    Idle,
    Pending,
    Complete,
}

fn poll_op<IO>(
    session: &Session<'_, IO>,
    op: &mut Option<Op>,
) -> Result<PendingOp, Error<IO::Error>>
where
    IO: Io,
{
    let Some(current) = *op else {
        return Ok(PendingOp::Idle);
    };
    match session.status(&current) {
        OpStatus::Pending => Ok(PendingOp::Pending),
        OpStatus::Complete => {
            *op = None;
            Ok(PendingOp::Complete)
        }
        OpStatus::Invalidated => Err(Error::Mqtt(MqttError::Disconnected)),
    }
}

#[derive(Serialize)]
struct AlivePayload {
    epoch: u32,
    schema_rev: u32,
    pages: usize,
}

pub(crate) enum PublishPayload<'a, 'b, Settings> {
    // Keep MM2 publications behind one concrete payload type per Settings tree.
    // `Session::publish<P>()` is generic over `P: ToPayload`; splitting these variants into
    // separate payload structs creates separate publish monomorphizations for alive/schema/leaf.
    Alive(&'a Manifest),
    SchemaPage {
        defs: &'a SchemaDefs,
        next: usize,
        hash: u32,
        advanced: &'b mut Option<(usize, u32)>,
    },
    Leaf {
        settings: &'a Settings,
        state: &'a [usize],
    },
}

fn serialize_leaf<Settings: TreeSerialize>(
    settings: &Settings,
    mut keys: &[usize],
    buf: &mut [u8],
) -> Result<usize, EncodeError<DepthError<JsonSerError>>> {
    let len = keys.len();
    json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| {
        let no_space = matches!(inner, SerdeError::Inner(JsonSerError::BufferFull));
        let err = DepthError {
            inner,
            depth: len - keys.len(),
        };
        (no_space, err)
    })
}

impl<Settings> ToPayload for PublishPayload<'_, '_, Settings>
where
    Settings: TreeSerialize,
{
    type Error = EncodeError<PayloadError>;

    fn serialize(self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Alive(manifest) => serde_json_core::to_slice(
                &AlivePayload {
                    epoch: manifest.epoch,
                    schema_rev: manifest.schema_rev,
                    pages: manifest.schema_pages,
                },
                buf,
            )
            .map_err(|err| {
                (
                    matches!(err, JsonSerError::BufferFull),
                    PayloadError::Json(err),
                )
            }),
            Self::SchemaPage {
                defs,
                next,
                hash,
                advanced,
            } => {
                let page = serialize_schema_page(defs, next, buf)
                    .map_err(|id| (true, PayloadError::Schema(id)))?;
                let next_hash = yafnv::Fnv::fnv1a(hash, buf[..page.len].iter().copied());
                *advanced = Some((page.count, next_hash));
                Ok(page.len)
            }
            Self::Leaf { settings, state } => serialize_leaf(settings, state, buf)
                .map_err(|(no_space, err)| (no_space, PayloadError::Leaf(err))),
        }
    }
}

/// Result of `Miniconf::serve()`.
#[must_use = "match on the event to handle unhandled traffic or a changed leaf"]
pub enum Event<T> {
    /// One non-MM2 inbound publish was returned through the callback.
    Unhandled(T),
    /// One `/set` changed this exact leaf and MM2 follow-up work completed.
    Changed(ChangedKey),
}

/// Immediate outcome of cooperative MM2 service work.
#[must_use = "match on the event to handle unhandled traffic or changed local settings"]
pub enum ServiceEvent {
    /// No immediate MM2 work or inbound publish was available.
    Idle,
    /// One MM2 request was rejected because bounded service capacity was exhausted.
    Busy,
    /// The message is not MM2 traffic.
    Unhandled,
    /// One `/set` changed this exact leaf and follow-up work was queued.
    Changed(ChangedKey),
}

enum Route {
    Unhandled,
    Ignored,
    Rejected {
        follow_up: Option<FollowUp>,
    },
    Accepted {
        changed: ChangedKey,
        follow_up: FollowUp,
    },
}

/// MM2 protocol state for one prefix and one settings tree.
pub struct Miniconf<Settings> {
    pub(crate) prefix: TopicString,
    pub(crate) manifest: Manifest,
    _settings: PhantomData<Settings>,
}

/// MM2 startup workflow for one MQTT connection event.
#[must_use = "drive startup to completion before relying on MM2 startup state"]
pub struct Startup {
    phase: sync::StartupPhase,
}

/// Cold-boot retained-settings load workflow.
///
/// Run this after the device process boots and connects, before its first connected [`Startup`],
/// when retained `settings/#` should seed the live settings tree.
///
/// Do not run this on network reconnects of a still-running device. Local settings are already the
/// authority in that case, even when the broker reports `ConnectEvent::Connected` because the MQTT
/// session was not resumed.
#[must_use = "drive retained loading to completion before starting MM2 publication"]
pub struct LoadRetained {
    phase: sync::LoadRetainedPhase,
}

/// Explicit retained publication workflow for a leaf, subtree, or root.
#[must_use = "drive the publisher to completion to update the retained subtree"]
pub struct Publisher {
    schema: &'static Schema,
    root: ChangedKey,
    iter: Option<SettingsSync>,
    pending: Option<ChangedKey>,
    op: Option<Op>,
}

/// Bounded cooperative MM2 service.
///
/// Use this when you want to interleave MM2 request handling with unrelated work while keeping the
/// number of queued MM2 follow-up publications bounded.
pub struct Service<const N: usize = 4> {
    follow_ups: Deque<FollowUp, N>,
}

pub(crate) fn schema_page_topic(prefix: &TopicString, page: usize) -> TopicString {
    let mut topic = prefix.clone();
    topic.push_str("/schema/").ok();
    use core::fmt::Write as _;
    write!(topic.as_mut_view(), "{page}").ok();
    topic
}

pub(crate) async fn publish_alive_once<Settings, IO>(
    prefix: &TopicString,
    manifest: &Manifest,
    session: &mut Session<'_, IO>,
) -> Result<Option<Op>, Error<IO::Error>>
where
    Settings: TreeSerialize,
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/alive")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    crate::debug!(
        "Publishing retained alive topic={=str} epoch={=u32} schema_rev={=u32} pages={=usize}",
        topic.as_str(),
        manifest.epoch,
        manifest.schema_rev,
        manifest.schema_pages
    );
    let publication = Publication::new(&topic, PublishPayload::<Settings>::Alive(manifest))
        .properties(crate::RETAINED_TEXT_PROPERTIES)
        .qos(QoS::AtLeastOnce)
        .retain();
    session.publish(publication).await.map_err(simple_pub_error)
}

impl<Settings> Miniconf<Settings>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
{
    /// Construct MM2 state and a configured caller-owned MQTT session.
    pub fn new<'buf, IO: Io>(
        prefix: &str,
        config: ConfigBuilder<'buf>,
    ) -> Result<(Self, Session<'buf, IO>), ConfigError> {
        let schema = Settings::SCHEMA;
        const { assert!(Settings::SCHEMA.max_depth() <= crate::MAX_DEPTH) }
        if prefix.len() + "/settings".len() + schema.max_length("/") > MAX_TOPIC_LENGTH {
            return Err(ConfigError::InvalidConfig);
        }
        if SchemaDefs::new(schema).is_err() {
            return Err(ConfigError::InvalidConfig);
        }

        let prefix: TopicString = prefix.try_into().map_err(|_| ConfigError::InvalidConfig)?;
        let mut will_topic = prefix.clone();
        will_topic
            .push_str("/alive")
            .map_err(|_| ConfigError::InvalidConfig)?;
        let will = Will::new(will_topic, b"", crate::RETAINED_TEXT_PROPERTIES)?
            .retained()
            .qos(QoS::AtLeastOnce);
        let config = config.autodowngrade_qos().will(will)?;
        let session = Session::new(config);

        Ok((
            Self {
                prefix,
                manifest: Manifest::default(),
                _settings: PhantomData,
            },
            session,
        ))
    }

    /// Run MM2 startup to completion after one MQTT connect event.
    ///
    /// `ConnectEvent::Connected` republishes schema/settings, subscribes `set/#`, and publishes
    /// `alive`. `ConnectEvent::Reconnected` only republishes `alive` because the MQTT session kept
    /// subscriptions and queued QoS state.
    ///
    /// This is the simple unbounded startup path. Fresh startup may discard inbound publishes
    /// while bootstrapping and is not the bounded/cancel-safe API.
    pub async fn startup<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &Settings,
        event: ConnectEvent,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: Io,
    {
        let mut startup = Startup::new(self, event);
        startup.run(self, session, settings).await
    }

    fn route(&mut self, settings: &mut Settings, inbound: &InboundPublish<'_>) -> Route {
        request::route(self.prefix.as_str(), settings, inbound)
    }

    /// Wait until one `/set` completes or one non-MM2 inbound publish is returned.
    ///
    /// This is the simple unbounded steady-state helper.
    ///
    /// `on_unhandled` runs synchronously for the first non-MM2 inbound publish and its return
    /// value is returned as `Event::Unhandled`.
    ///
    /// This callback is the ownership boundary for the borrowed MQTT receive buffer. Returning
    /// `InboundPublish<'_>` directly from this unbounded helper would make the same async loop both
    /// return a borrow from `session` and reborrow `session` to complete MM2 follow-up work.
    ///
    /// For async application work, copy or extract the needed data in `on_unhandled`, return it
    /// through `Event::Unhandled`, and await after `serve()` returns.
    ///
    /// This helper favors simplicity over exact control:
    /// - it is unbounded
    /// - it may discard unrelated inbound publishes that arrive while completing MM2 follow-up
    ///   work
    /// - use [`Service`] when you need bounded stepwise control
    pub async fn serve<IO, T>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &mut Settings,
        on_unhandled: impl FnOnce(&InboundPublish<'_>) -> T,
    ) -> Result<Event<T>, Error<IO::Error>>
    where
        IO: Io,
    {
        // Reuse the bounded service path with capacity one. `serve()` drains every queued
        // follow-up before polling another request, so `Busy` is not reachable in normal use.
        let mut service = Service::<1>::new();
        loop {
            let inbound = session.poll().await?;
            let Some(inbound) = inbound else {
                continue;
            };
            match service.handle(self, settings, &inbound) {
                ServiceEvent::Unhandled => {
                    return Ok(Event::Unhandled(on_unhandled(&inbound)));
                }
                ServiceEvent::Idle | ServiceEvent::Busy => {
                    while !service.step(self, session, settings).await? {
                        let _ = session.poll().await?;
                    }
                }
                ServiceEvent::Changed(changed) => {
                    while !service.step(self, session, settings).await? {
                        let _ = session.poll().await?;
                    }
                    return Ok(Event::Changed(changed));
                }
            }
        }
    }

    pub(crate) fn settings_topic(&self, state: &[usize]) -> Result<TopicString, ResourceError> {
        let path: ConstPath<TopicString, '/'> = Settings::SCHEMA
            .transcode(state)
            .map_err(|_| ResourceError::BufferTooSmall)?;
        let mut topic = self.prefix.clone();
        topic
            .push_str("/settings")
            .map_err(|_| ResourceError::BufferTooSmall)?;
        topic
            .push_str(path.as_ref())
            .map_err(|_| ResourceError::BufferTooSmall)?;
        Ok(topic)
    }

    pub(crate) async fn publish_current<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &Settings,
        state: &[usize],
    ) -> Result<Option<Op>, Error<IO::Error>>
    where
        IO: Io,
    {
        let topic = self
            .settings_topic(state)
            .map_err(MqttError::from)
            .map_err(Error::from)?;
        crate::debug!(
            "Publishing authoritative setting topic={=str}",
            topic.as_str()
        );

        let props = [
            Property::PayloadFormatIndicator(1),
            Property::UserProperty(Utf8String("auth"), Utf8String("")),
        ];
        let publication = Publication::new(&topic, PublishPayload::Leaf { settings, state })
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        match session.publish(publication).await {
            Ok(op) => Ok(op),
            Err(PubError::Payload((
                _no_space,
                PayloadError::Leaf(DepthError {
                    inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                    ..
                }),
            ))) => {
                crate::debug!(
                    "Clearing authoritative setting topic={=str}",
                    topic.as_str()
                );
                let publication = Publication::bytes(&topic, b"")
                    .properties(&props)
                    .qos(QoS::AtLeastOnce)
                    .retain();
                let op = session
                    .publish(publication)
                    .await
                    .map_err(simple_pub_error)?;
                Ok(op)
            }
            Err(err) => Err(simple_pub_error(err)),
        }
    }
}

impl Startup {
    /// Begin MM2 startup after one MQTT connect event.
    pub fn new<Settings>(mm2: &mut Miniconf<Settings>, event: ConnectEvent) -> Self
    where
        Settings: TreeSchema,
    {
        match event {
            ConnectEvent::Connected => Self::connected(mm2),
            ConnectEvent::Reconnected => {
                crate::info!(
                    "Starting reconnected MM2 startup prefix={=str} epoch={=u32} schema_rev={=u32}",
                    mm2.prefix.as_str(),
                    mm2.manifest.epoch,
                    mm2.manifest.schema_rev
                );
                Self::reconnected()
            }
        }
    }

    /// Begin the MM2 startup path for `ConnectEvent::Connected`.
    ///
    /// Use this after cold-boot [`LoadRetained`] so the retained mirror is republished
    /// authoritatively from the recovered local settings.
    pub fn connected<Settings>(mm2: &mut Miniconf<Settings>) -> Self
    where
        Settings: TreeSchema,
    {
        mm2.manifest.epoch = mm2.manifest.epoch.wrapping_add(1);
        mm2.manifest.schema_rev = 0;
        mm2.manifest.schema_pages = 0;
        crate::info!(
            "Starting connected MM2 startup prefix={=str} epoch={=u32}",
            mm2.prefix.as_str(),
            mm2.manifest.epoch
        );
        Self {
            phase: sync::StartupPhase::Schema {
                sync: SchemaSync::new(Settings::SCHEMA),
                op: None,
            },
        }
    }

    fn reconnected() -> Self {
        Self {
            phase: sync::StartupPhase::Alive(None),
        }
    }

    /// Run MM2 startup to completion.
    ///
    /// This helper may consume and discard surfaced inbound publishes while bootstrapping.
    pub async fn run<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        while !self.step(mm2, session, settings).await? {
            let _ = session.poll().await?;
        }
        Ok(())
    }

    /// Advance MM2 startup.
    ///
    /// `Ok(true)` means startup is complete.
    ///
    /// `Ok(false)` means no more immediate startup progress is possible. Wait for later session
    /// progress, then call `step()` again.
    ///
    /// Connected-session startup may discard surfaced inbound publishes while bootstrapping.
    pub async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        self.phase.step(mm2, session, settings).await
    }
}

impl Default for LoadRetained {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadRetained {
    /// Begin loading retained `settings/#` into local settings.
    pub fn new() -> Self {
        Self {
            phase: sync::LoadRetainedPhase::new(),
        }
    }

    /// Run retained settings load to completion.
    ///
    /// This helper owns the temporary `settings/#` subscription while it runs. Use it only before
    /// the first MM2 startup of a device process. Afterwards run [`Startup::connected`] to publish
    /// the recovered settings authoritatively.
    pub async fn run<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &mut Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        while !self.step(mm2, session, settings).await? {}
        Ok(())
    }

    /// Advance retained settings loading.
    ///
    /// `Ok(true)` means retained loading is complete and the temporary subscription has been
    /// removed.
    ///
    /// This workflow consumes inbound publishes while draining the retained `settings/#` burst.
    pub async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &mut Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        self.phase.step(mm2, session, settings).await
    }
}

impl Publisher {
    /// Begin explicit retained publication for the whole tree root.
    pub fn root(schema: &'static Schema) -> Self {
        Self {
            schema,
            root: ChangedKey::new([0; crate::MAX_DEPTH], 0),
            iter: None,
            pending: None,
            op: None,
        }
    }

    /// Begin explicit retained publication for one leaf or subtree.
    pub fn by_key(schema: &'static Schema, key: impl IntoKeys) -> Result<Self, ResolveError> {
        let mut state = [0; crate::MAX_DEPTH];
        let lookup = schema.resolve_into(key, &mut state)?;
        Ok(Self {
            schema,
            root: ChangedKey::new(state, lookup.depth),
            iter: None,
            pending: None,
            op: None,
        })
    }

    /// Run retained subtree publication to completion.
    ///
    /// This is the simple unbounded helper. It may discard unrelated inbound publishes while
    /// waiting for MQTT session progress. Use [`Publisher::step`] when those publishes must be
    /// routed elsewhere.
    pub async fn run<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        while !self.step(mm2, session, settings).await? {
            let _ = session.poll().await?;
        }
        Ok(())
    }

    /// Advance retained subtree publication.
    ///
    /// `Ok(true)` means publication is complete.
    ///
    /// `Ok(false)` means no more immediate publication progress is possible. Wait for later
    /// session progress, then call `step()` again.
    ///
    /// This method never consumes unrelated inbound publishes.
    pub async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        sync::step_publisher(self, mm2, session, settings).await
    }
}

impl<const N: usize> Default for Service<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Service<N> {
    /// Construct an empty bounded MM2 service.
    pub const fn new() -> Self {
        Self {
            follow_ups: Deque::new(),
        }
    }

    /// Return whether no queued MM2 follow-up work remains.
    pub fn is_empty(&self) -> bool {
        self.follow_ups.is_empty()
    }

    /// Return the number of queued MM2 follow-up workflows.
    pub fn len(&self) -> usize {
        self.follow_ups.len()
    }

    fn is_full(&self) -> bool {
        self.follow_ups.len() == N
    }

    /// Route one inbound publish through the bounded MM2 service.
    ///
    /// Non-MM2 traffic is reported as `ServiceEvent::Unhandled`, while the
    /// caller keeps ownership of the inbound publish.
    ///
    /// If the bounded service is full, MM2 `/set` requests are rejected without mutating local
    /// settings.
    pub fn handle<Settings>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        settings: &mut Settings,
        inbound: &InboundPublish<'_>,
    ) -> ServiceEvent
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    {
        if self.is_full() && request::needs_capacity::<Settings>(mm2.prefix.as_str(), inbound) {
            crate::debug!(
                "Rejecting MM2 request because service backlog is full topic={=str} queued={=usize} capacity={=usize} payload_len={=usize}",
                inbound.topic(),
                self.follow_ups.len(),
                N,
                inbound.payload().len()
            );
            return ServiceEvent::Busy;
        }

        match mm2.route(settings, inbound) {
            Route::Unhandled => ServiceEvent::Unhandled,
            Route::Ignored => ServiceEvent::Idle,
            Route::Rejected { follow_up } => {
                if let Some(follow_up) = follow_up {
                    debug_assert!(!self.is_full());
                    let _ = self.follow_ups.push_back(follow_up);
                    crate::debug!(
                        "Queued MM2 error follow-up queued={=usize} capacity={=usize}",
                        self.follow_ups.len(),
                        N
                    );
                }
                ServiceEvent::Idle
            }
            Route::Accepted { changed, follow_up } => {
                debug_assert!(!self.is_full());
                let _ = self.follow_ups.push_back(follow_up);
                crate::debug!(
                    "Queued MM2 publish follow-up changed_depth={=usize} queued={=usize} capacity={=usize}",
                    changed.as_ref().len(),
                    self.follow_ups.len(),
                    N
                );
                ServiceEvent::Changed(changed)
            }
        }
    }

    /// Advance one queued MM2 follow-up workflow.
    ///
    /// `Ok(true)` means no queued MM2 follow-up work remains after this step.
    ///
    /// `Ok(false)` means queued work remains and later session progress is needed before calling
    /// `step()` again.
    ///
    /// This method never consumes unrelated inbound publishes.
    pub async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        loop {
            let Some(mut follow_up) = self.follow_ups.pop_front() else {
                return Ok(true);
            };
            crate::debug!(
                "Driving MM2 follow-up queued_before={=usize} capacity={=usize}",
                self.follow_ups.len() + 1,
                N
            );

            if follow_up.step(mm2, session, settings).await? {
                crate::debug!(
                    "Completed MM2 follow-up queued_remaining={=usize} capacity={=usize}",
                    self.follow_ups.len(),
                    N
                );
                continue;
            }
            let _ = self.follow_ups.push_front(follow_up);
            crate::debug!(
                "MM2 follow-up pending queued_remaining={=usize} capacity={=usize}",
                self.follow_ups.len(),
                N
            );
            return Ok(false);
        }
    }
}

pub(crate) type ReplyTarget = OwnedResponseTarget<MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH>;
