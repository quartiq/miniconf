mod request;
mod sync;

use core::marker::PhantomData;

use heapless::Deque;
use miniconf::{
    DescendError, Indices, IntoKeys, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    json_core,
};
use minimq::{
    ConfigBuilder, Error as MqttError, InboundPublish, Io, Op, OpStatus, OwnedResponseTarget,
    Property, ProtocolError, PubError, Publication, QoS, Session, TopicString, Will,
    publication::ToPayload, types::Utf8String,
};
use serde::Serialize;

use crate::{
    EncodeError, MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH,
    message::DepthError,
    schema::{SchemaDefs, serialize_schema_page},
};

/// Exact leaf indices produced by MM2 request handling.
pub type ChangedKey = Indices<[usize; crate::MAX_DEPTH]>;

#[derive(Debug, PartialEq, thiserror::Error)]
/// MM2 protocol error.
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
    pub(crate) settings_rev: u32,
}

#[derive(Debug)]
pub(crate) enum PayloadError {
    Json(serde_json_core::ser::Error),
    Schema(usize),
    Leaf(DepthError<serde_json_core::ser::Error>),
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
) -> Result<usize, EncodeError<DepthError<serde_json_core::ser::Error>>> {
    let len = keys.len();
    json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| {
        let no_space = matches!(
            inner,
            miniconf::SerdeError::Inner(serde_json_core::ser::Error::BufferFull)
        );
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
                    matches!(err, serde_json_core::ser::Error::BufferFull),
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

#[allow(clippy::large_enum_variant)]
/// Result of handling one inbound publish.
#[must_use = "match on the result and drive any returned response to completion"]
pub enum Handle<'a> {
    /// The message is not MM2 traffic and remains owned by the caller.
    Unhandled(InboundPublish<'a>),
    /// MM2 handled the message and intentionally ignored it.
    Ignored,
    /// MM2 rejected the request.
    Rejected {
        /// Optional reply work for the rejected request.
        response: Option<Response>,
    },
    /// MM2 accepted the request and already changed local settings.
    Accepted {
        /// The exact leaf changed locally by the successful `/set`.
        changed: ChangedKey,
        /// Required follow-up work for the accepted request.
        ///
        /// This always exists because the authoritative retained `settings/...` update still has
        /// to be published, even without a reply topic.
        response: Response,
    },
}

/// Result of `Miniconf::poll_with()`.
#[must_use = "match on the event to handle unhandled traffic or a changed leaf"]
pub enum Event<T> {
    /// One non-MM2 inbound publish was returned through the callback.
    Unhandled(T),
    /// One `/set` changed this exact leaf and MM2 follow-up work completed.
    Changed(ChangedKey),
}

/// Immediate outcome of routing one inbound publish.
#[must_use = "match on the event to handle unhandled traffic or changed local settings"]
pub enum Ingress<T> {
    /// The message is not MM2 traffic and remains owned by the caller.
    Unhandled(T),
    /// MM2 handled the message and intentionally ignored it.
    Ignored,
    /// MM2 rejected the request and any required reply work was queued.
    Rejected,
    /// MM2 accepted the request, changed local settings, and queued required follow-up work.
    Accepted(ChangedKey),
}

/// Outcome of attempting to queue MM2 request aftermath work.
#[allow(clippy::large_enum_variant)]
#[must_use = "match on the outcome to handle ingress or recover the original Handle"]
pub enum QueueResult<'a> {
    /// Any required MM2 follow-up work was queued successfully.
    Ingress(Ingress<InboundPublish<'a>>),
    /// The queue could not accept the follow-up work, so the original `Handle` is returned
    /// unchanged.
    Full(Handle<'a>),
}

/// MM2 protocol state for one prefix and one settings tree.
pub struct Miniconf<Settings> {
    pub(crate) prefix: TopicString,
    pub(crate) manifest: Manifest,
    _settings: PhantomData<Settings>,
}

/// Fresh-session MM2 bootstrap workflow.
#[must_use = "drive activation to completion before relying on MM2 startup state"]
pub struct Activation {
    phase: sync::ActivationPhase,
}

/// Explicit retained publication workflow for a leaf, subtree, or root.
#[must_use = "drive the publisher to completion to update the retained subtree"]
pub struct Publisher {
    root: ChangedKey,
    iter: Option<crate::schema::SettingsSync>,
    pending: Option<ChangedKey>,
    op: Option<Op>,
}

/// Effectful aftermath of one handled `/set` request.
#[must_use = "drive the response to completion to publish retained state and any requested reply"]
pub struct Response {
    phase: request::ResponsePhase,
}

/// Bounded queue of MM2 request aftermath work.
///
/// Use this when you want to keep accepting new `/set` requests while earlier `Response`s are
/// still being driven to completion.
pub struct ResponseQueue<const N: usize = 4> {
    responses: Deque<Response, N>,
}

impl<'a> Handle<'a> {
    /// Queue any required MM2 response work and return the immediate ingress outcome.
    ///
    /// If the queue cannot accept the follow-up work, the original `Handle` is returned
    /// unchanged.
    pub fn queue_into<const N: usize>(self, queue: &mut ResponseQueue<N>) -> QueueResult<'a> {
        match self {
            Self::Unhandled(inbound) => QueueResult::Ingress(Ingress::Unhandled(inbound)),
            Self::Ignored => QueueResult::Ingress(Ingress::Ignored),
            Self::Rejected { response } => {
                if let Some(response) = response {
                    return match queue.responses.push_back(response) {
                        Ok(()) => QueueResult::Ingress(Ingress::Rejected),
                        Err(response) => QueueResult::Full(Self::Rejected {
                            response: Some(response),
                        }),
                    };
                }
                QueueResult::Ingress(Ingress::Rejected)
            }
            Self::Accepted { changed, response } => match queue.responses.push_back(response) {
                Ok(()) => QueueResult::Ingress(Ingress::Accepted(changed)),
                Err(response) => QueueResult::Full(Self::Accepted { changed, response }),
            },
        }
    }
}

impl<Settings> Miniconf<Settings>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
{
    async fn complete_response<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &Settings,
        mut response: Response,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: Io,
    {
        while !response.step(self, session, settings).await? {
            let _ = session.poll().await?;
        }
        Ok(())
    }

    async fn complete_publisher<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &Settings,
        mut publisher: Publisher,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: Io,
    {
        while !publisher.step(self, session, settings).await? {
            let _ = session.poll().await?;
        }
        Ok(())
    }

    fn with_leaf<T, E>(
        full: &[usize],
        func: impl FnOnce(&mut &[usize]) -> Result<T, SerdeError<E>>,
    ) -> Result<T, DepthError<E>> {
        let mut keys = full;
        func(&mut keys).map_err(|inner| DepthError {
            inner,
            depth: full.len() - keys.len(),
        })
    }

    /// Construct MM2 state and a configured caller-owned MQTT session.
    pub fn new<'buf, IO: Io>(
        prefix: &str,
        config: ConfigBuilder<'buf>,
    ) -> Result<(Self, Session<'buf, IO>), ProtocolError> {
        const { assert!(Settings::SCHEMA.max_depth() <= crate::MAX_DEPTH) }
        if prefix.len() + "/settings".len() + Settings::SCHEMA.max_length("/") > MAX_TOPIC_LENGTH {
            return Err(ProtocolError::BufferSize);
        }
        if SchemaDefs::new(Settings::SCHEMA).is_err() {
            return Err(ProtocolError::BufferSize);
        }

        let prefix: TopicString = prefix.try_into().map_err(|_| ProtocolError::BufferSize)?;
        let mut will_topic = prefix.clone();
        will_topic
            .push_str("/alive")
            .map_err(|_| ProtocolError::BufferSize)?;
        let will = Will::new(will_topic, b"", &[])?
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

    /// Begin fresh-session MM2 bootstrap.
    ///
    /// Call this after `Session::connect()` returns `ConnectEvent::Connected`.
    pub fn begin_activation(&mut self) -> Activation {
        self.manifest.epoch = self.manifest.epoch.wrapping_add(1);
        self.manifest.settings_rev = 0;
        self.manifest.schema_rev = 0;
        self.manifest.schema_pages = 0;
        Activation::new::<Settings>()
    }

    /// Run fresh-session MM2 bootstrap to completion.
    ///
    /// This is the simple unbounded activation path. It may discard inbound publishes while
    /// bootstrapping and is not the bounded/cancel-safe API.
    pub async fn activate<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: Io,
    {
        let mut activation = self.begin_activation();
        while !activation.step(self, session, settings).await? {
            let _ = session.poll().await?;
        }
        Ok(())
    }

    /// Publish retained `alive` after a resumed MQTT session.
    ///
    /// Call this after `Session::connect()` returns `ConnectEvent::Reconnected`.
    pub async fn publish_alive<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: Io,
    {
        self.publish_alive_once(session).await.map(|_| ())
    }

    /// Parse and apply one inbound publish synchronously.
    ///
    /// Non-MM2 traffic is returned as `Handle::Unhandled`.
    ///
    /// Successful `/set` requests mutate `settings` immediately and return `Handle::Accepted`.
    /// Rejected requests return `Handle::Rejected`.
    pub fn handle<'msg>(
        &mut self,
        settings: &mut Settings,
        inbound: InboundPublish<'msg>,
    ) -> Handle<'msg> {
        request::handle::<Settings>(self.prefix.as_str(), settings, inbound)
    }

    /// Wait until one `/set` completes or one non-MM2 inbound publish is returned.
    ///
    /// This is the simple unbounded steady-state helper.
    ///
    /// `on_unhandled` runs synchronously for the first non-MM2 inbound publish and its return
    /// value is returned as `Event::Unhandled`.
    ///
    /// This helper favors simplicity over exact control:
    /// - it is unbounded
    /// - it may discard unrelated inbound publishes that arrive while completing MM2 follow-up
    ///   work
    /// - use `Session::recv()`, `Miniconf::handle()`, and `Response::step()` directly when that
    ///   is not acceptable
    pub async fn poll_with<IO, T>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &mut Settings,
        mut on_unhandled: impl FnMut(InboundPublish<'_>) -> T,
    ) -> Result<Event<T>, Error<IO::Error>>
    where
        IO: Io,
    {
        loop {
            let inbound = session.poll().await?;
            let Some(inbound) = inbound else {
                continue;
            };
            match self.handle(settings, inbound) {
                Handle::Unhandled(message) => return Ok(Event::Unhandled(on_unhandled(message))),
                Handle::Ignored => {}
                Handle::Rejected { response } => {
                    if let Some(response) = response {
                        self.complete_response(session, settings, response).await?;
                    }
                }
                Handle::Accepted { changed, response } => {
                    self.complete_response(session, settings, response).await?;
                    return Ok(Event::Changed(changed));
                }
            }
        }
    }

    /// Begin explicit retained publication for the whole tree root.
    pub fn publish_root(&self) -> Publisher {
        Publisher {
            root: ChangedKey::new([0; crate::MAX_DEPTH], 0),
            iter: None,
            pending: None,
            op: None,
        }
    }

    /// Begin explicit retained publication for one leaf or subtree.
    pub fn publish_by_key(&self, key: impl IntoKeys) -> Result<Publisher, miniconf::ResolveError> {
        let mut state = [0; crate::MAX_DEPTH];
        let lookup = Settings::SCHEMA.resolve_into(key, &mut state)?;
        Ok(Publisher {
            root: ChangedKey::new(state, lookup.depth),
            iter: None,
            pending: None,
            op: None,
        })
    }

    pub(crate) async fn publish_alive_once<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
    ) -> Result<Option<Op>, Error<IO::Error>>
    where
        IO: Io,
    {
        let mut topic = self.prefix.clone();
        topic
            .push_str("/alive")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let publication =
            Publication::new(&topic, PublishPayload::<Settings>::Alive(&self.manifest))
                .qos(QoS::AtLeastOnce)
                .retain();
        session
            .publish(publication)
            .await
            .map_err(crate::message::simple_pub_error)
    }

    pub(crate) fn schema_page_topic(&self, page: usize) -> TopicString {
        let mut topic = self.prefix.clone();
        topic.push_str("/schema/").ok();
        use core::fmt::Write as _;
        write!(&mut topic, "{page}").ok();
        topic
    }

    pub(crate) fn settings_topic(&self, state: &[usize]) -> Result<TopicString, ProtocolError> {
        let path: miniconf::ConstPath<TopicString, '/'> = Settings::SCHEMA
            .transcode(state)
            .map_err(|_| ProtocolError::BufferSize)?;
        let mut topic = self.prefix.clone();
        topic
            .push_str("/settings")
            .map_err(|_| ProtocolError::BufferSize)?;
        topic
            .push_str(path.as_ref())
            .map_err(|_| ProtocolError::BufferSize)?;
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
            .map_err(|err| Error::Mqtt(err.into()))?;
        match self.try_publish_leaf(session, settings, state).await {
            Ok(op) => Ok(op),
            Err(PubError::Payload((
                _no_space,
                PayloadError::Leaf(DepthError {
                    inner:
                        miniconf::SerdeError::Value(
                            miniconf::ValueError::Absent | miniconf::ValueError::Access(_),
                        ),
                    ..
                }),
            ))) => self.clear_leaf(session, &topic).await,
            Err(err) => Err(crate::message::simple_pub_error(err)),
        }
    }

    pub(crate) async fn try_publish_leaf<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        settings: &Settings,
        state: &[usize],
    ) -> Result<Option<Op>, PubError<EncodeError<PayloadError>, IO::Error>>
    where
        IO: Io,
    {
        let topic = self.settings_topic(state).map_err(PubError::from)?;
        let next_rev = self.manifest.settings_rev.wrapping_add(1);
        let mut rev = itoa::Buffer::new();
        let props = [Property::UserProperty(
            Utf8String("rev"),
            Utf8String(rev.format(next_rev)),
        )];
        let publication = Publication::new(&topic, PublishPayload::Leaf { settings, state })
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        let op = session.publish(publication).await?;
        self.manifest.settings_rev = next_rev;
        Ok(op)
    }

    pub(crate) async fn clear_leaf<IO>(
        &mut self,
        session: &mut Session<'_, IO>,
        topic: &str,
    ) -> Result<Option<Op>, Error<IO::Error>>
    where
        IO: Io,
    {
        let next_rev = self.manifest.settings_rev.wrapping_add(1);
        let mut rev = itoa::Buffer::new();
        let props = [Property::UserProperty(
            Utf8String("rev"),
            Utf8String(rev.format(next_rev)),
        )];
        let publication = Publication::bytes(topic, b"")
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        let op = session
            .publish(publication)
            .await
            .map_err(crate::message::simple_pub_error)?;
        self.manifest.settings_rev = next_rev;
        Ok(op)
    }
}

impl<const N: usize> Default for ResponseQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ResponseQueue<N> {
    /// Construct an empty bounded MM2 response queue.
    pub const fn new() -> Self {
        Self {
            responses: Deque::new(),
        }
    }

    /// Return whether no queued MM2 response work remains.
    pub fn is_empty(&self) -> bool {
        self.responses.is_empty()
    }

    /// Return the number of queued MM2 response workflows.
    pub fn len(&self) -> usize {
        self.responses.len()
    }

    /// Parse and queue one inbound publish.
    ///
    /// Successful `/set` requests mutate `settings` immediately. Any required MM2 follow-up work
    /// is queued and must later be driven with [`step`](Self::step).
    ///
    /// If the queue cannot accept that follow-up work, the original `Handle` is returned
    /// unchanged.
    pub fn handle<'msg, Settings>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        settings: &mut Settings,
        inbound: InboundPublish<'msg>,
    ) -> QueueResult<'msg>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    {
        mm2.handle(settings, inbound).queue_into(self)
    }

    /// Advance queued MM2 aftermath work.
    ///
    /// `Ok(true)` means the queue is empty after this step.
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
        let Some(mut response) = self.responses.pop_front() else {
            return Ok(true);
        };

        if response.step(mm2, session, settings).await? {
            Ok(self.responses.is_empty())
        } else {
            let _ = self.responses.push_front(response);
            Ok(false)
        }
    }
}

impl Activation {
    fn new<Settings: TreeSchema>() -> Self {
        Self {
            phase: sync::ActivationPhase::Schema(sync::SchemaPublisher::new::<Settings>()),
        }
    }

    /// Advance fresh-session MM2 bootstrap.
    ///
    /// `Ok(true)` means bootstrap is complete.
    ///
    /// `Ok(false)` means no more immediate bootstrap progress is possible. Wait for later session
    /// progress, then call `step()` again.
    ///
    /// This method may discard surfaced inbound publishes while bootstrapping.
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

impl Publisher {
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

    /// Run retained subtree publication to completion.
    ///
    /// This is the simple unbounded publication path. It may discard inbound publishes that
    /// arrive while waiting for later session progress.
    pub async fn complete<Settings, IO>(
        self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        mm2.complete_publisher(session, settings, self).await
    }
}

impl Response {
    /// Advance one handled-request aftermath.
    ///
    /// `Ok(true)` means the aftermath is complete.
    ///
    /// `Ok(false)` means no more immediate progress is possible. Wait for later session progress,
    /// then call `step()` again.
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
        self.phase.step(mm2, session, settings).await
    }

    /// Run one handled-request aftermath to completion.
    ///
    /// This is the simple unbounded response path. It may discard inbound publishes that arrive
    /// while waiting for later session progress.
    pub async fn complete<Settings, IO>(
        self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        mm2.complete_response(session, settings, self).await
    }
}
pub(crate) type ReplyTarget = OwnedResponseTarget<MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH>;
