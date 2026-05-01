mod request;
mod sync;

use core::{future::poll_fn, marker::PhantomData, task::Poll};

use heapless::String;
use miniconf::{
    DescendError, Indices, IntoKeys, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    json_core,
};
use minimq::{
    ConfigBuilder, InboundPublish, OwnedResponseTarget, ProtocolError, PubError, Publication, QoS,
    Session, publication::ToPayload,
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
    Mqtt(#[from] minimq::Error<E>),
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
    state: &[usize],
    buf: &mut [u8],
) -> Result<usize, EncodeError<DepthError<serde_json_core::ser::Error>>> {
    let mut keys = state;
    json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| {
        let err = DepthError {
            inner,
            depth: state.len() - keys.len(),
        };
        let no_space = matches!(
            err.inner,
            miniconf::SerdeError::Inner(serde_json_core::ser::Error::BufferFull)
        );
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

/// MM2 protocol state for one prefix and one settings tree.
pub struct Miniconf<'a, Settings> {
    pub(crate) prefix: &'a str,
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
}

/// Effectful aftermath of one handled `/set` request.
#[must_use = "drive the response to completion to publish retained state and any requested reply"]
pub struct Response {
    phase: request::ResponsePhase,
}

impl<'a, Settings> Miniconf<'a, Settings>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
{
    async fn yield_once() {
        let mut yielded = false;
        poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await
    }

    async fn complete_response<IO>(
        &mut self,
        session: &mut Session<'a, IO>,
        settings: &Settings,
        mut response: Response,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        while !response.step(self, session, settings).await? {
            if session.drive().await?.is_none() {
                Self::yield_once().await;
            }
        }
        Ok(())
    }

    async fn complete_publisher<IO>(
        &mut self,
        session: &mut Session<'a, IO>,
        settings: &Settings,
        mut publisher: Publisher,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        while !publisher.step(self, session, settings).await? {
            if session.drive().await?.is_none() {
                Self::yield_once().await;
            }
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
    pub fn new<IO: minimq::Io>(
        prefix: &'a str,
        config: ConfigBuilder<'a>,
    ) -> Result<(Self, Session<'a, IO>), ProtocolError> {
        const { assert!(Settings::SCHEMA.max_depth() <= crate::MAX_DEPTH) }
        if prefix.len() + "/settings".len() + Settings::SCHEMA.max_length("/") > MAX_TOPIC_LENGTH {
            return Err(ProtocolError::BufferSize);
        }
        if SchemaDefs::new(Settings::SCHEMA).is_err() {
            return Err(ProtocolError::BufferSize);
        }

        let mut will_topic: String<MAX_TOPIC_LENGTH> =
            prefix.try_into().map_err(|_| ProtocolError::BufferSize)?;
        will_topic
            .push_str("/alive")
            .map_err(|_| ProtocolError::BufferSize)?;
        let will = minimq::Will::owned(&will_topic, b"", &[])?
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
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        let mut activation = self.begin_activation();
        while !activation.step(self, session, settings).await? {
            Self::yield_once().await;
        }
        Ok(())
    }

    /// Publish retained `alive` after a resumed MQTT session.
    ///
    /// Call this after `Session::connect()` returns `ConnectEvent::Reconnected`.
    pub async fn publish_alive<IO>(
        &mut self,
        session: &mut Session<'a, IO>,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        self.publish_alive_once(session).await
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
        request::handle::<Settings>(self.prefix, settings, inbound)
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
    /// - use `Session::poll()`, `Miniconf::handle()`, and `Response::step()` directly when that
    ///   is
    ///   not acceptable
    pub async fn poll_with<IO, T>(
        &mut self,
        session: &mut Session<'a, IO>,
        settings: &mut Settings,
        mut on_unhandled: impl FnMut(InboundPublish<'_>) -> T,
    ) -> Result<Event<T>, Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        loop {
            let inbound = session.poll().await?;
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
        })
    }

    pub(crate) async fn publish_alive_once<IO>(
        &mut self,
        session: &mut Session<'a, IO>,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
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

    pub(crate) fn schema_page_topic(&self, page: usize) -> String<MAX_TOPIC_LENGTH> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        topic.push_str("/schema/").ok();
        use core::fmt::Write as _;
        write!(&mut topic, "{page}").ok();
        topic
    }

    pub(crate) fn settings_topic<IO>(
        &self,
        state: &[usize],
    ) -> Result<String<MAX_TOPIC_LENGTH>, Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        let path: miniconf::ConstPath<String<MAX_TOPIC_LENGTH>, '/'> = Settings::SCHEMA
            .transcode(state)
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/settings")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str(path.as_ref())
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        Ok(topic)
    }

    pub(crate) async fn publish_current<IO>(
        &mut self,
        session: &mut Session<'a, IO>,
        settings: &Settings,
        state: &[usize],
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        let topic = self.settings_topic::<IO>(state)?;
        match self.try_publish_leaf(session, settings, state).await {
            Ok(()) => Ok(()),
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
        session: &mut Session<'a, IO>,
        settings: &Settings,
        state: &[usize],
    ) -> Result<(), PubError<EncodeError<PayloadError>, IO::Error>>
    where
        IO: minimq::Io,
    {
        let topic = self.settings_topic::<IO>(state).map_err(|err| match err {
            Error::Mqtt(err) => PubError::Session(err),
            Error::Tree(_) => unreachable!(),
        })?;
        let next_rev = self.manifest.settings_rev.wrapping_add(1);
        let mut rev = itoa::Buffer::new();
        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("rev"),
            minimq::types::Utf8String(rev.format(next_rev)),
        )];
        let publication = Publication::new(&topic, PublishPayload::Leaf { settings, state })
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        session.publish(publication).await?;
        self.manifest.settings_rev = next_rev;
        Ok(())
    }

    pub(crate) async fn clear_leaf<IO>(
        &mut self,
        session: &mut Session<'a, IO>,
        topic: &str,
    ) -> Result<(), Error<IO::Error>>
    where
        IO: minimq::Io,
    {
        let next_rev = self.manifest.settings_rev.wrapping_add(1);
        let mut rev = itoa::Buffer::new();
        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("rev"),
            minimq::types::Utf8String(rev.format(next_rev)),
        )];
        let publication = Publication::bytes(topic, b"")
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        session
            .publish(publication)
            .await
            .map_err(crate::message::simple_pub_error)?;
        self.manifest.settings_rev = next_rev;
        Ok(())
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
    /// This method may drive one bounded MQTT progress step internally and may discard surfaced
    /// inbound publishes.
    pub async fn step<'a, Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<'a, Settings>,
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: minimq::Io,
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
    pub async fn step<'a, Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<'a, Settings>,
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: minimq::Io,
    {
        sync::step_publisher(self, mm2, session, settings).await
    }

    /// Run retained subtree publication to completion.
    ///
    /// This is the simple unbounded publication path. It may discard inbound publishes that
    /// arrive while waiting for later session progress.
    pub async fn complete<'a, Settings, IO>(
        self,
        mm2: &mut Miniconf<'a, Settings>,
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: minimq::Io,
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
    pub async fn step<'a, Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<'a, Settings>,
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: minimq::Io,
    {
        self.phase.step(mm2, session, settings).await
    }

    /// Run one handled-request aftermath to completion.
    ///
    /// This is the simple unbounded response path. It may discard inbound publishes that arrive
    /// while waiting for later session progress.
    pub async fn complete<'a, Settings, IO>(
        self,
        mm2: &mut Miniconf<'a, Settings>,
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: minimq::Io,
    {
        mm2.complete_response(session, settings, self).await
    }
}

pub(crate) type ReplyTarget = OwnedResponseTarget<MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH>;
