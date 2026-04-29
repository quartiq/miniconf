mod request;
mod sync;

use core::marker::PhantomData;

use embassy_time::{Duration, Instant, with_deadline};
use heapless::String;
use log::{debug, info};
use miniconf::{
    DescendError, IntoKeys, KeyError, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    json_core,
};
use minimq::{
    ConfigBuilder, ConnectEvent, InboundPublish, Property, ProtocolError,
    PubError, Publication, QoS, Session,
    publication::ToPayload,
    types::{SubscriptionOptions, TopicFilter},
};
use serde::Serialize;

#[cfg(feature = "compat-settings-ingress")]
use crate::message::Resource;
use crate::{
    EncodeError, MAX_TOPIC_LENGTH,
    message::{Action, DepthError},
    schema::{SchemaDefs, serialize_schema_page},
};

fn ignore_other(_: &InboundPublish<'_>) {}

const BACKGROUND_POLL_SLICE: Duration = Duration::from_millis(1);

#[derive(Debug, PartialEq, thiserror::Error)]
/// MM2 MQTT client error.
pub enum Error<E> {
    /// Static path resolution failed before touching the value.
    #[error("miniconf path resolution failed: {0}")]
    Miniconf(DescendError<()>),
    /// MQTT session or publication failure.
    #[error(transparent)]
    Mqtt(#[from] minimq::Error<E>),
}

impl<E> From<DescendError<()>> for Error<E> {
    fn from(value: DescendError<()>) -> Self {
        Self::Miniconf(value)
    }
}

#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Change {
    #[default]
    Unchanged,
    Changed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// One app-visible outcome from [`MqttClient::connect`] or [`MqttClient::poll`].
pub enum Event {
    /// An MM2 request updated at least one setting leaf.
    Changed,
    /// The broker created a fresh MQTT/MM2 session.
    Connected,
    /// The broker resumed the existing MQTT/MM2 session.
    Reconnected,
    /// A non-MM2 inbound publish was delivered to the callback.
    Other,
}

#[derive(Default)]
pub(super) struct Manifest {
    epoch: u32,
    schema_rev: u32,
    schema_pages: usize,
    settings_rev: u32,
}

#[derive(Debug)]
pub(super) enum PayloadError {
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

pub(super) enum PublishPayload<'a, 'b, Settings> {
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
    let full = state;
    let mut keys = full;
    json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| {
        let err = DepthError {
            inner,
            depth: full.len() - keys.len(),
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

struct ProtocolState {
    manifest: Manifest,
    settings_sync_requested: u32,
    settings_sync_completed: u32,
}

impl ProtocolState {
    fn new() -> Self {
        Self {
            manifest: Manifest::default(),
            settings_sync_requested: 0,
            settings_sync_completed: 0,
        }
    }

    fn on_session_active(&mut self, reconnected: bool) {
        if reconnected {
            info!("Reconnected MM2 session");
            return;
        }

        self.manifest.epoch = self.manifest.epoch.wrapping_add(1);
        self.manifest.settings_rev = 0;
        self.manifest.schema_rev = 0;
        self.manifest.schema_pages = 0;
        self.settings_sync_requested = 0;
        self.settings_sync_completed = 0;
        info!("Activated MM2 session epoch={}", self.manifest.epoch);
    }

    #[cfg(feature = "compat-settings-ingress")]
    fn request_settings_sync(&mut self) {
        self.settings_sync_requested = self.settings_sync_requested.wrapping_add(1);
    }

    fn settings_sync_pending(&self) -> bool {
        self.settings_sync_requested != self.settings_sync_completed
    }
}

/// MM2 MQTT session wrapper for one Miniconf tree.
pub struct MqttClient<'a, Settings, IO> {
    session: Session<'a, IO>,
    prefix: &'a str,
    protocol: ProtocolState,
    _settings: PhantomData<Settings>,
}

impl<'a, Settings, IO> MqttClient<'a, Settings, IO>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    IO: minimq::Io,
{
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

    /// Construct a new MM2 MQTT client for one Miniconf settings tree.
    pub fn new(prefix: &'a str, config: ConfigBuilder<'a>) -> Result<Self, ProtocolError> {
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

        Ok(Self {
            session: Session::new(config),
            prefix,
            protocol: ProtocolState::new(),
            _settings: PhantomData,
        })
    }

    /// Wait for one app-visible MM2 outcome on an already-connected session.
    ///
    /// This does not own connect/reconnect or full retained schema/settings publication. Call
    /// [`connect`](Self::connect) first. If the underlying MQTT session disconnects, `poll()`
    /// returns `Error::Mqtt(minimq::Error::Disconnected)` and the caller decides when to
    /// reconnect.
    ///
    /// Guaranteed cancel-safe only if [`is_poll_cancel_safe`](Self::is_poll_cancel_safe) is true
    /// when called. Otherwise cancellation can interrupt MM2 request handling or deferred MM2
    /// follow-up work, though later calls will resume deferred full settings resync.
    pub async fn poll(
        &mut self,
        settings: &mut Settings,
        mut on_other: impl FnMut(&InboundPublish<'_>),
    ) -> Result<Event, Error<IO::Error>> {
        self.require_connected()?;
        self.flush_pending_settings_sync(settings, &mut on_other)
            .await?;
        loop {
            let message = self.session.poll().await.map_err(Error::from)?;
            match Self::plan_inbound(self.prefix, settings, &message, &mut on_other) {
                None => return Ok(Event::Other),
                Some(action) => match self.execute(settings, action).await {
                    Change::Unchanged => continue,
                    Change::Changed => {
                        self.flush_pending_settings_sync(settings, &mut on_other)
                            .await?;
                        return Ok(Event::Changed);
                    }
                },
            }
        }
    }

    /// Establish or resume the MQTT/MM2 session on a new transport.
    ///
    /// This performs the underlying MQTT handshake plus MM2 setup:
    /// request-topic subscriptions, optional compatibility ingress recovery, and the fresh-session
    /// retained manifest/schema/settings publication pass.
    ///
    /// Cancel safety:
    /// the underlying MQTT `connect()` handshake is cancel-safe in the sense documented by
    /// `minimq`, but this higher-level MM2 activation sequence is not. Cancelling this method can
    /// leave a connected session with partially completed MM2 bootstrap work.
    pub async fn connect(
        &mut self,
        io: IO,
        settings: &mut Settings,
    ) -> Result<Event, Error<IO::Error>> {
        let reconnected = match self.session.connect(io).await.map_err(Error::from)? {
            ConnectEvent::Connected => false,
            ConnectEvent::Reconnected => true,
        };
        self.protocol.on_session_active(reconnected);
        let mut on_other = ignore_other;
        if reconnected {
            debug!("Publishing alive manifest");
            self.publish_alive_once().await?;
            return Ok(Event::Reconnected);
        }
        #[cfg(feature = "compat-settings-ingress")]
        {
            self.subscribe_topic_suffix("/settings/#").await?;
            debug!("Subscribed compat settings topic");
            self.recover_settings_ingress(settings).await?;
        }
        self.publish_schema(settings, &mut on_other).await?;
        self.publish_settings(settings, &mut on_other).await?;
        self.publish_alive(settings, &mut on_other).await?;
        self.flush_pending_settings_sync(settings, &mut on_other)
            .await?;
        self.subscribe_topic_suffix("/set/#").await?;
        debug!("Subscribed set request topic");
        Ok(Event::Connected)
    }

    /// Publish one retained leaf value by exact key.
    ///
    /// This is the efficient app-side hook for a known leaf change. If the key resolves to an
    /// internal node, use [`publish_all`](Self::publish_all) after the structural change instead.
    ///
    /// Not fully cancel-safe: cancellation can advance local MM2 revision tracking without
    /// completing the retained publication.
    pub async fn publish_by_key(
        &mut self,
        settings: &Settings,
        key: impl IntoKeys,
    ) -> Result<(), Error<IO::Error>> {
        let mut state = [0; crate::MAX_DEPTH];
        let lookup = Settings::SCHEMA
            .resolve_into(key, &mut state)
            .map_err(|err| err.error)?;
        if !lookup.schema.is_leaf() {
            return Err(Error::Miniconf(DescendError::Key(KeyError::TooShort)));
        }
        self.require_connected()?;
        self.publish_current(settings, &state[..lookup.depth]).await
    }

    /// Publish the full retained MM2 schema/settings mirror.
    ///
    /// This is explicit and unbounded, like [`connect`](Self::connect).
    /// It is not fully cancel-safe because cancellation can leave the retained MM2 mirror only
    /// partially republished.
    pub async fn publish_all(
        &mut self,
        settings: &mut Settings,
        mut on_other: impl for<'msg> FnMut(&InboundPublish<'msg>),
    ) -> Result<(), Error<IO::Error>> {
        self.require_connected()?;
        self.publish_schema(settings, &mut on_other).await?;
        self.publish_settings(settings, &mut on_other).await?;
        self.publish_alive(settings, &mut on_other).await?;
        self.flush_pending_settings_sync(settings, &mut on_other)
            .await?;
        Ok(())
    }

    /// Subscribe additional non-MM2 topics on the shared session.
    ///
    /// The caller owns these subscriptions. Re-subscribe after [`connect`](Self::connect)
    /// returns [`Event::Connected`].
    ///
    /// Cancel-safe if the underlying transport I/O futures are cancel-safe.
    pub async fn subscribe(
        &mut self,
        topics: &[TopicFilter<'_>],
        properties: &[Property<'_>],
    ) -> Result<(), Error<IO::Error>> {
        self.require_connected()?;
        self.session.subscribe(topics, properties).await?;
        Ok(())
    }

    /// Unsubscribe additional non-MM2 topics from the shared session.
    ///
    /// Cancel-safe if the underlying transport I/O futures are cancel-safe.
    pub async fn unsubscribe(
        &mut self,
        topics: &[&str],
        properties: &[Property<'_>],
    ) -> Result<(), Error<IO::Error>> {
        self.require_connected()?;
        self.session.unsubscribe(topics, properties).await?;
        Ok(())
    }

    /// Whether the MQTT session can currently publish at the requested QoS.
    pub fn can_publish(&mut self, qos: QoS) -> bool {
        self.session.can_publish(qos)
    }

    /// Whether the underlying MQTT session has no in-flight retained publish/release work.
    ///
    /// This mirrors the underlying `minimq` transport/session quiescence only. It does not include
    /// deferred MM2 follow-up work such as a full retained settings resync.
    pub fn is_publish_quiescent(&self) -> bool {
        self.session.is_publish_quiescent()
    }

    /// Whether calling [`poll`](Self::poll) is guaranteed cancel-safe at this instant.
    ///
    /// If this is `true`, `poll()` will only wait in the cancel-safe blocking
    /// `minimq::Session::poll()` path until a new inbound publish arrives or the session is lost.
    /// If this is `false`, `poll()` may first perform deferred MM2 follow-up work such as a full
    /// retained settings resync.
    pub fn is_poll_cancel_safe(&self) -> bool {
        !self.protocol.settings_sync_pending()
    }

    /// Publish an arbitrary MQTT packet after MM2 activation.
    ///
    /// Inherits `minimq` cancel safety: QoS 1/2 publications are cancel-safe if the underlying
    /// transport I/O futures are cancel-safe; QoS 0 publications are not.
    pub async fn publish<P>(
        &mut self,
        publication: Publication<'_, P>,
    ) -> Result<(), PubError<P::Error, IO::Error>>
    where
        P: ToPayload,
    {
        self.require_connected().map_err(|err| match err {
            Error::Mqtt(err) => PubError::Session(err),
            Error::Miniconf(_) => unreachable!(),
        })?;
        self.session.publish(publication).await
    }

    fn require_connected(&self) -> Result<(), Error<IO::Error>> {
        if self.session.is_connected() {
            Ok(())
        } else {
            Err(Error::Mqtt(minimq::Error::Disconnected))
        }
    }

    fn plan_inbound<F>(
        prefix: &str,
        settings: &mut Settings,
        message: &InboundPublish<'_>,
        on_other: &mut F,
    ) -> Option<Action>
    where
        F: for<'msg> FnMut(&InboundPublish<'msg>),
    {
        let action = Self::plan_request(prefix, settings, message);
        if matches!(action, Action::Unhandled) {
            on_other(message);
            return None;
        }
        Some(action)
    }

    async fn poll_quiescent<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&InboundPublish<'msg>),
    {
        let deadline = Instant::now() + BACKGROUND_POLL_SLICE;
        match with_deadline(deadline, self.session.poll()).await {
            Ok(Ok(message)) => {
                if let Some(action) = Self::plan_inbound(self.prefix, settings, &message, on_other)
                {
                    let _ = self.execute(settings, action).await;
                }
            }
            Ok(Err(err)) => return Err(Error::from(err)),
            Err(_) => {}
        }
        Ok(())
    }

    async fn flush_pending_settings_sync<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&InboundPublish<'msg>),
    {
        while self.protocol.settings_sync_pending() {
            let target = self.protocol.settings_sync_requested;
            self.publish_settings(settings, on_other).await?;
            self.protocol.settings_sync_completed = target;
        }
        Ok(())
    }

    async fn subscribe_topic_suffix(&mut self, suffix: &str) -> Result<(), Error<IO::Error>> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str(suffix)
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let topics = [TopicFilter::new(&topic)
            .options(SubscriptionOptions::default().ignore_local_messages())];
        self.session.subscribe(&topics, &[]).await?;
        Ok(())
    }

    #[cfg(feature = "compat-settings-ingress")]
    async fn recover_settings_ingress(
        &mut self,
        settings: &mut Settings,
    ) -> Result<(), Error<IO::Error>> {
        let mut deadline = Instant::now() + crate::SETTINGS_RECOVERY_QUIESCENCE;
        debug!("Starting settings ingress recovery");
        loop {
            match with_deadline(deadline, self.session.poll()).await {
                Ok(Ok(message)) => {
                    let Some((Resource::Settings, _)) =
                        Resource::parse(message.topic(), self.prefix)
                    else {
                        continue;
                    };
                    deadline = Instant::now() + crate::SETTINGS_RECOVERY_QUIESCENCE;
                    if let Some(action) =
                        Self::plan_settings_recovery(self.prefix, settings, &message)
                    {
                        let _ = self.execute_settings_recovery(action);
                    }
                }
                Ok(Err(err)) => return Err(Error::from(err)),
                Err(_) => {
                    debug!("Finished settings ingress recovery");
                    return Ok(());
                }
            }
        }
    }
}
