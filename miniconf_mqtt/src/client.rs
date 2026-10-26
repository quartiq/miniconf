mod request;
mod sync;

use core::marker::PhantomData;

use heapless::Deque;
use miniconf::{
    ConstPath, DescendError, Indices, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    ValueError,
    compact_schema::{SchemaDefs, serialize_schema_page},
    json_core,
};
use minimq::{
    ConfigBuilder, ConfigError, Connection, Error as MqttError, InboundPublish, Io, Op,
    OwnedResponseTarget, Property, Publication, QoS, ResourceError, Session, ToPayload, Will,
};
use serde::Serialize;

use crate::{
    MAX_DEPTH, MAX_SCHEMA_DEFS, MAX_TOPIC_LENGTH, PROTOCOL_VERSION, RESPONSE_CORRELATION_LENGTH,
    RETAINED_TEXT_PROPERTIES, TopicString, debug, message::simple_pub_error,
};
use request::FollowUp;
pub use sync::{LoadRetained, Publisher, Startup};

/// Exact leaf indices produced by Miniconf request handling.
pub type ChangedKey = Indices<[usize; MAX_DEPTH]>;

#[derive(Debug, PartialEq, thiserror::Error)]
/// Miniconf MQTT setup, tree, or MQTT connection error.
pub enum Error<E> {
    /// Tree traversal or path resolution failed before any MQTT I/O.
    #[error("tree path resolution failed: {0}")]
    Tree(DescendError<()>),
    /// MQTT connection or publication failure.
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
    Serialize,
    Schema(usize),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PendingOp {
    Idle,
    Pending,
    Complete,
}

fn is_backpressure<IO: Io>(connection: &Connection<'_, '_, IO>, error: &Error<IO::Error>) -> bool {
    match error {
        Error::Mqtt(
            MqttError::NotReady | MqttError::Resource(ResourceError::InflightExhausted),
        ) => true,
        Error::Mqtt(MqttError::Resource(ResourceError::BufferTooSmall)) => {
            !connection.session().is_publish_quiescent()
        }
        _ => false,
    }
}

fn poll_op<IO>(
    connection: &Connection<'_, '_, IO>,
    op: &mut Option<Op>,
) -> Result<PendingOp, Error<IO::Error>>
where
    IO: Io,
{
    let Some(current) = *op else {
        return Ok(PendingOp::Idle);
    };
    if connection.is_pending(&current) {
        Ok(PendingOp::Pending)
    } else if connection.is_complete(&current) {
        *op = None;
        Ok(PendingOp::Complete)
    } else {
        debug_assert!(connection.is_invalidated(&current));
        Err(Error::Mqtt(MqttError::Disconnected))
    }
}

#[derive(Serialize)]
struct AlivePayload {
    proto: u8,
    epoch: u32,
    schema_rev: u32,
    pages: usize,
}

pub(crate) enum PublishPayload<'a, 'b, Settings> {
    // Keep Miniconf MQTT publications behind one concrete payload type per Settings tree.
    // `Connection::publish<P>()` is generic over `P: ToPayload`; splitting these variants into
    // separate payload structs creates separate publish monomorphizations for alive/schema/leaf.
    Alive(&'a Manifest),
    SchemaPage {
        defs: &'a SchemaDefs<{ MAX_SCHEMA_DEFS }>,
        next: usize,
        hash: u32,
        advanced: &'b mut Option<(usize, u32)>,
    },
    Leaf {
        settings: &'a Settings,
        state: &'a [usize],
    },
}

impl<Settings> ToPayload for PublishPayload<'_, '_, Settings>
where
    Settings: TreeSerialize,
{
    type Error = PayloadError;

    fn serialize(self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Alive(manifest) => serde_json_core::to_slice(
                &AlivePayload {
                    proto: PROTOCOL_VERSION,
                    epoch: manifest.epoch,
                    schema_rev: manifest.schema_rev,
                    pages: manifest.schema_pages,
                },
                buf,
            )
            .map_err(|_| PayloadError::Serialize),
            Self::SchemaPage {
                defs,
                next,
                hash,
                advanced,
            } => {
                let page = serialize_schema_page(defs, next, buf).map_err(PayloadError::Schema)?;
                let next_hash = yafnv::Fnv::fnv1a(hash, buf[..page.len].iter().copied());
                *advanced = Some((page.count, next_hash));
                Ok(page.len)
            }
            Self::Leaf { settings, state } => match json_core::get_by_keys(settings, state, buf) {
                Ok(len) => Ok(len),
                Err(SerdeError::Value(ValueError::Absent | ValueError::Access(_))) => Ok(0),
                Err(_) => Err(PayloadError::Serialize),
            },
        }
    }
}

/// Result of `Miniconf::serve()`.
#[must_use = "match on the event to handle unhandled traffic or a changed leaf"]
pub enum Event<T> {
    /// One non-Miniconf inbound publish was returned through the callback.
    Unhandled(T),
    /// This leaf was successfully set and protocol follow-up work completed.
    Changed(ChangedKey),
}

/// Immediate outcome of cooperative Miniconf service work.
#[must_use = "match on the event to handle unhandled traffic or changed local settings"]
pub enum ServiceEvent {
    /// The message was consumed without reporting a settings change; follow-up work may be queued.
    Idle,
    /// One Miniconf request was rejected because bounded service capacity was exhausted.
    Busy,
    /// The message is not Miniconf traffic.
    Unhandled,
    /// This leaf was successfully set and follow-up work was queued.
    Changed(ChangedKey),
}

enum Route {
    Unhandled,
    Ignored,
    Busy,
    Rejected {
        follow_up: Option<FollowUp>,
    },
    Accepted {
        changed: ChangedKey,
        follow_up: FollowUp,
    },
}

/// Miniconf MQTT protocol state for one prefix and one settings tree.
pub struct Miniconf<Settings> {
    pub(crate) prefix: TopicString,
    pub(crate) manifest: Manifest,
    pub(crate) startup_complete: bool,
    _settings: PhantomData<Settings>,
}

/// Bounded cooperative Miniconf service.
///
/// Use this when you want to interleave Miniconf request handling with unrelated work while keeping
/// the number of queued protocol follow-up publications bounded.
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
    connection: &mut Connection<'_, '_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    Settings: TreeSerialize,
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/alive")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    debug!(
        "Publishing retained alive topic={=str} epoch={=u32} schema_rev={=u32} pages={=usize}",
        topic.as_str(),
        manifest.epoch,
        manifest.schema_rev,
        manifest.schema_pages
    );
    let publication = Publication::new(&topic, PublishPayload::<Settings>::Alive(manifest))
        .properties(RETAINED_TEXT_PROPERTIES)
        .qos(QoS::AtLeastOnce)
        .retain();
    connection
        .publish(publication)
        .await
        .map_err(simple_pub_error)?
        .ok_or(Error::Mqtt(MqttError::InvalidRequest))
}

impl<Settings> Miniconf<Settings>
where
    Settings: TreeSchema,
{
    /// Construct Miniconf MQTT state and a configured caller-owned MQTT session.
    ///
    /// The broker must support QoS 1. Do not enable automatic QoS downgrade on `config`:
    /// startup and service completion depend on broker publication acknowledgements.
    pub fn new<'buf>(
        prefix: &str,
        config: ConfigBuilder<'buf>,
    ) -> Result<(Self, Session<'buf>), ConfigError> {
        let schema = Settings::SCHEMA;
        const { assert!(Settings::SCHEMA.max_depth() <= MAX_DEPTH) }
        if prefix.len() + "/settings".len() + schema.max_length("/") > MAX_TOPIC_LENGTH {
            return Err(ConfigError::InvalidConfig);
        }
        if SchemaDefs::<{ MAX_SCHEMA_DEFS }>::new(schema).is_err() {
            return Err(ConfigError::InvalidConfig);
        }

        let prefix: TopicString = prefix.try_into().map_err(|_| ConfigError::InvalidConfig)?;
        let mut will_topic = prefix.clone();
        will_topic
            .push_str("/alive")
            .map_err(|_| ConfigError::InvalidConfig)?;
        let will = Will::new(will_topic.as_str(), b"", RETAINED_TEXT_PROPERTIES)?
            .retained()
            .qos(QoS::AtLeastOnce);
        let config = config.will(will)?;
        let session = Session::new(config);

        Ok((
            Self {
                prefix,
                manifest: Manifest::default(),
                startup_complete: false,
                _settings: PhantomData,
            },
            session,
        ))
    }

    /// Run Miniconf startup to completion after one MQTT connect event.
    ///
    /// `ConnectEvent::Connected` republishes schema/settings, subscribes `set/#`, and publishes
    /// `alive`. `ConnectEvent::Reconnected` only republishes `alive` if startup previously
    /// completed; otherwise it restarts synchronization from the current settings.
    ///
    /// This is the simple unbounded startup path. Fresh startup may discard inbound publishes
    /// while bootstrapping and is not the bounded/cancel-safe API.
    pub async fn startup<IO>(
        &mut self,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        let mut startup = Startup::new(self, connection.connect_event());
        startup.run(self, connection, settings).await
    }

    /// Wait until one leaf write completes or one non-Miniconf inbound publish is returned.
    ///
    /// `on_unhandled` runs synchronously for the first non-Miniconf inbound publish and its return
    /// value is returned as `Event::Unhandled`.
    ///
    /// For async application work, copy or extract the needed data in `on_unhandled`, return it
    /// through `Event::Unhandled`, and await after `serve()` returns.
    ///
    /// This unbounded helper may discard unrelated inbound publishes while completing follow-up
    /// work. Cancellation drops its pending follow-ups; use [`Service`] to retain them between calls
    /// and route unrelated traffic yourself.
    pub async fn serve<IO, T>(
        &mut self,
        connection: &mut Connection<'_, '_, IO>,
        settings: &mut Settings,
        on_unhandled: impl FnOnce(&InboundPublish<'_>) -> T,
    ) -> Result<Event<T>, Error<IO::Error>>
    where
        Settings: TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        // Reuse the bounded service path with capacity one. `serve()` drains every queued
        // follow-up before polling another request, so `Busy` is not reachable in normal use.
        let mut service = Service::<1>::new();
        loop {
            let inbound = connection.poll().await?;
            let Some(inbound) = inbound else {
                continue;
            };
            let event = service.handle(self, settings, &inbound);
            if matches!(event, ServiceEvent::Unhandled) {
                return Ok(Event::Unhandled(on_unhandled(&inbound)));
            }
            while !service.step(self, connection, settings).await? {
                let _ = connection.poll().await?;
            }
            if let ServiceEvent::Changed(changed) = event {
                return Ok(Event::Changed(changed));
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
        &self,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
        state: &[usize],
    ) -> Result<Op, Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        let topic = self
            .settings_topic(state)
            .map_err(MqttError::from)
            .map_err(Error::from)?;
        debug!(
            "Publishing authoritative setting topic={=str}",
            topic.as_str()
        );

        let props = [
            Property::PayloadFormatIndicator(1),
            Property::UserProperty("auth", ""),
        ];
        let publication = Publication::new(&topic, PublishPayload::Leaf { settings, state })
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        connection
            .publish(publication)
            .await
            .map_err(simple_pub_error)?
            .ok_or(Error::Mqtt(MqttError::InvalidRequest))
    }
}

impl<const N: usize> Default for Service<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> Service<N> {
    /// Construct an empty bounded Miniconf service.
    pub const fn new() -> Self {
        Self {
            follow_ups: Deque::new(),
        }
    }

    /// Return whether no queued protocol follow-up work remains.
    pub fn is_empty(&self) -> bool {
        self.follow_ups.is_empty()
    }

    /// Return the number of queued protocol follow-up workflows.
    pub fn len(&self) -> usize {
        self.follow_ups.len()
    }

    /// Route one inbound publish through the bounded Miniconf service.
    ///
    /// Non-Miniconf traffic is reported as `ServiceEvent::Unhandled`, while the
    /// caller keeps ownership of the inbound publish.
    ///
    /// If the bounded service is full, mutation requests are rejected without changing local
    /// settings.
    pub fn handle<Settings>(
        &mut self,
        miniconf: &Miniconf<Settings>,
        settings: &mut Settings,
        inbound: &InboundPublish<'_>,
    ) -> ServiceEvent
    where
        Settings: TreeDeserializeOwned,
    {
        match request::route(
            miniconf.prefix.as_str(),
            settings,
            inbound,
            self.follow_ups.is_full(),
        ) {
            Route::Unhandled => ServiceEvent::Unhandled,
            Route::Ignored => ServiceEvent::Idle,
            Route::Busy => {
                debug!(
                    "Rejecting Miniconf request because service backlog is full topic={=str} queued={=usize} capacity={=usize} payload_len={=usize}",
                    inbound.topic(),
                    self.follow_ups.len(),
                    N,
                    inbound.payload().len()
                );
                ServiceEvent::Busy
            }
            Route::Rejected { follow_up } => {
                if let Some(follow_up) = follow_up {
                    debug_assert!(!self.follow_ups.is_full());
                    let _ = self.follow_ups.push_back(follow_up);
                }
                ServiceEvent::Idle
            }
            Route::Accepted { changed, follow_up } => {
                debug_assert!(!self.follow_ups.is_full());
                let _ = self.follow_ups.push_back(follow_up);
                ServiceEvent::Changed(changed)
            }
        }
    }

    /// Advance one queued protocol follow-up workflow.
    ///
    /// `Ok(true)` means no queued protocol follow-up work remains after this step.
    ///
    /// `Ok(false)` means queued work remains and later connection progress is needed before calling
    /// `step()` again.
    ///
    /// This method never consumes unrelated inbound publishes.
    /// Cancelling it retains the current follow-up for the next call. Transport cancellation
    /// safety still depends on the underlying I/O; retries may duplicate queued packets.
    /// An error discards the failed follow-up.
    pub async fn step<Settings, IO>(
        &mut self,
        miniconf: &Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        loop {
            let Some(follow_up) = self.follow_ups.front_mut() else {
                return Ok(true);
            };

            let result = follow_up.step(miniconf, connection, settings).await;
            if !matches!(result, Ok(false)) {
                self.follow_ups.pop_front();
                result?;
                continue;
            }
            return Ok(false);
        }
    }
}

pub(crate) type ReplyTarget = OwnedResponseTarget<MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH>;
