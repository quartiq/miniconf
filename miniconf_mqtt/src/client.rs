mod request;
mod sync;

use core::marker::PhantomData;

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Instant;
use embassy_time::Timer;
use heapless::String;
use log::{debug, info};
use miniconf::{
    DescendError, IntoKeys, KeyError, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
};
use minimq::{
    ConfigBuilder, ConnectEvent, Event as SessionEvent, InboundPublish, Property, ProtocolError,
    PubError, Publication, QoS, Session,
    publication::ToPayload,
    types::{SubscriptionOptions, TopicFilter},
};

#[cfg(feature = "compat-settings-ingress")]
use crate::message::Resource;
use crate::{
    MAX_TOPIC_LENGTH,
    message::{Action, DepthError},
    schema::SchemaDefs,
};

fn ignore_other(_: &InboundPublish<'_>) {}

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

#[cfg(feature = "compat-settings-ingress")]
#[derive(Copy, Clone)]
enum SettingsIngressPhase {
    Recovering,
    Runtime,
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
    /// No app-visible event occurred.
    Idle,
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
struct Manifest {
    epoch: u32,
    schema_rev: u32,
    schema_pages: usize,
    settings_rev: u32,
}

struct ProtocolState {
    manifest: Manifest,
    pending_settings_sync: bool,
    #[cfg(feature = "compat-settings-ingress")]
    settings_ingress: SettingsIngressPhase,
    #[cfg(feature = "compat-settings-ingress")]
    settings_ingress_deadline: Option<Instant>,
}

impl ProtocolState {
    fn new() -> Self {
        Self {
            manifest: Manifest::default(),
            pending_settings_sync: false,
            #[cfg(feature = "compat-settings-ingress")]
            settings_ingress: SettingsIngressPhase::Runtime,
            #[cfg(feature = "compat-settings-ingress")]
            settings_ingress_deadline: None,
        }
    }

    fn on_session_active(&mut self, reconnected: bool) {
        if reconnected {
            info!("Reconnected MM2 session");
            #[cfg(feature = "compat-settings-ingress")]
            {
                self.settings_ingress = SettingsIngressPhase::Runtime;
                self.settings_ingress_deadline = None;
            }
            return;
        }

        self.manifest.epoch = self.manifest.epoch.wrapping_add(1);
        self.manifest.settings_rev = 0;
        self.manifest.schema_rev = 0;
        self.manifest.schema_pages = 0;
        self.pending_settings_sync = false;
        info!("Activated MM2 session epoch={}", self.manifest.epoch);
        #[cfg(feature = "compat-settings-ingress")]
        {
            self.settings_ingress = SettingsIngressPhase::Recovering;
            self.settings_ingress_deadline = None;
            debug!("Starting settings ingress recovery");
        }
    }
}

/// MM2 MQTT session wrapper for one Miniconf tree.
pub struct MqttClient<'a, Settings, IO> {
    session: Session<'a, IO>,
    prefix: &'a str,
    set_subscribed: bool,
    #[cfg(feature = "compat-settings-ingress")]
    compat_subscribed: bool,
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
            set_subscribed: false,
            #[cfg(feature = "compat-settings-ingress")]
            compat_subscribed: false,
            protocol: ProtocolState::new(),
            _settings: PhantomData,
        })
    }

    /// Progress one MM2 step on an already-connected session.
    ///
    /// This does not own connect/reconnect or full retained schema/settings publication.
    /// Call [`connect`](Self::connect) first. If the
    /// underlying MQTT session disconnects,
    /// `poll()` returns `Error::Mqtt(minimq::Error::Disconnected)` and the caller decides when to
    /// reconnect.
    pub async fn poll(
        &mut self,
        settings: &mut Settings,
        mut on_other: impl FnMut(&InboundPublish<'_>),
    ) -> Result<Event, Error<IO::Error>> {
        self.require_connected()?;
        match self.session.poll().await.map_err(Error::from)? {
            SessionEvent::Idle => Ok(Event::Idle),
            SessionEvent::Inbound(message) => {
                let action = Self::plan_request(self.prefix, settings, &message);
                if matches!(action, Action::Unhandled) {
                    on_other(&message);
                    return Ok(Event::Other);
                }
                let event = match self.execute(settings, action).await {
                    Change::Unchanged => Event::Idle,
                    Change::Changed => Event::Changed,
                };
                self.flush_pending_settings_sync(settings, &mut on_other)
                    .await?;
                Ok(event)
            }
        }
    }

    /// Establish or resume the MQTT/MM2 session on a new transport.
    ///
    /// This performs the underlying MQTT handshake plus MM2 setup:
    /// request-topic subscriptions, optional compatibility ingress recovery, and the fresh-session
    /// retained manifest/schema/settings publication pass.
    pub async fn connect(
        &mut self,
        io: IO,
        settings: &mut Settings,
    ) -> Result<Event, Error<IO::Error>> {
        let reconnected = match self.session.connect(io).await.map_err(Error::from)? {
            ConnectEvent::Connected => false,
            ConnectEvent::Reconnected => true,
        };
        self.on_session_active(reconnected);
        let mut on_other = ignore_other;
        self.finish_connect(settings, reconnected, &mut on_other)
            .await?;
        Ok(if reconnected {
            Event::Reconnected
        } else {
            Event::Connected
        })
    }

    /// Publish one retained leaf value by exact key.
    ///
    /// This is the efficient app-side hook for a known leaf change. If the key resolves to an
    /// internal node, use [`publish_all`](Self::publish_all) after the structural change instead.
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
        self.publish_current(settings, state, lookup.depth).await
    }

    /// Publish the full retained MM2 schema/settings mirror.
    ///
    /// This is explicit and unbounded, like [`connect`](Self::connect).
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

    /// Whether app-owned publishes can proceed without contending with MM2
    /// protocol work.
    pub fn can_publish_app(&mut self, qos: QoS) -> bool {
        self.session.can_publish(qos)
    }

    fn require_connected(&self) -> Result<(), Error<IO::Error>> {
        if self.session.is_connected() {
            Ok(())
        } else {
            Err(Error::Mqtt(minimq::Error::Disconnected))
        }
    }

    async fn wait_publish_quiescent<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&InboundPublish<'msg>),
    {
        while !self.session.is_publish_quiescent() {
            match self.session.poll().await.map_err(Error::from)? {
                SessionEvent::Idle => Timer::after_millis(0).await,
                SessionEvent::Inbound(message) => {
                    let action = Self::plan_request(self.prefix, settings, &message);
                    if matches!(action, Action::Unhandled) {
                        on_other(&message);
                    } else {
                        let _ = self.execute(settings, action).await;
                    }
                }
            }
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
        while self.protocol.pending_settings_sync {
            self.protocol.pending_settings_sync = false;
            self.publish_settings(settings, on_other).await?;
        }
        Ok(())
    }

    async fn finish_connect<F>(
        &mut self,
        settings: &mut Settings,
        reconnected: bool,
        on_other: &mut F,
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&InboundPublish<'msg>),
    {
        #[cfg(feature = "compat-settings-ingress")]
        if !reconnected {
            self.subscribe_compat_requests().await?;
            self.recover_settings_ingress(settings).await?;
        }
        if reconnected {
            debug!("Publishing alive manifest");
            self.publish_alive_once().await?;
            return Ok(());
        }
        self.publish_schema(settings, on_other).await?;
        self.publish_settings(settings, on_other).await?;
        self.publish_alive(settings, on_other).await?;
        self.flush_pending_settings_sync(settings, on_other).await?;
        self.subscribe_set_requests().await?;
        Ok(())
    }

    async fn subscribe_set_requests(&mut self) -> Result<(), Error<IO::Error>> {
        if self.set_subscribed {
            return Ok(());
        }
        let topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let mut set = topic.clone();
        set.push_str("/set/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let topics = [TopicFilter::new(&set).options(opts)];
        self.session.subscribe(&topics, &[]).await?;
        self.set_subscribed = true;
        debug!("Subscribed set request topic");
        Ok(())
    }

    #[cfg(feature = "compat-settings-ingress")]
    async fn subscribe_compat_requests(&mut self) -> Result<(), Error<IO::Error>> {
        if self.compat_subscribed {
            return Ok(());
        }
        let topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let mut compat = topic;
        compat
            .push_str("/settings/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let topics = [TopicFilter::new(&compat).options(opts)];
        self.session.subscribe(&topics, &[]).await?;
        self.compat_subscribed = true;
        debug!("Subscribed compat settings topic");
        Ok(())
    }

    /// Publish an arbitrary MQTT packet after MM2 activation.
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

    fn on_session_active(&mut self, reconnected: bool) {
        if reconnected {
            self.set_subscribed = true;
            #[cfg(feature = "compat-settings-ingress")]
            {
                self.compat_subscribed = true;
            }
            self.protocol.on_session_active(true);
            return;
        }
        self.set_subscribed = false;
        #[cfg(feature = "compat-settings-ingress")]
        {
            self.compat_subscribed = false;
        }
        self.protocol.on_session_active(false);
    }

    #[cfg(feature = "compat-settings-ingress")]
    async fn recover_settings_ingress(
        &mut self,
        settings: &mut Settings,
    ) -> Result<(), Error<IO::Error>> {
        loop {
            let SettingsIngressPhase::Recovering = self.protocol.settings_ingress else {
                return Ok(());
            };
            if self
                .protocol
                .settings_ingress_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                self.protocol.settings_ingress = SettingsIngressPhase::Runtime;
                self.protocol.settings_ingress_deadline = None;
                debug!("Finished settings ingress recovery");
                return Ok(());
            }
            match self.session.poll().await.map_err(Error::from)? {
                SessionEvent::Idle => {
                    if self.protocol.settings_ingress_deadline.is_none() {
                        self.protocol.settings_ingress = SettingsIngressPhase::Runtime;
                        debug!("Finished settings ingress recovery without retained settings");
                        return Ok(());
                    }
                    Timer::after_millis(1).await
                }
                SessionEvent::Inbound(message) => {
                    if matches!(
                        Resource::parse(message.topic(), self.prefix),
                        Some((Resource::Settings, _))
                    ) {
                        self.protocol.settings_ingress_deadline =
                            Some(Instant::now() + crate::SETTINGS_RECOVERY_QUIESCENCE);
                    }
                    let action = Self::plan_request(self.prefix, settings, &message);
                    if !matches!(action, Action::Unhandled) {
                        let _ = self.execute(settings, action).await;
                    }
                }
            }
        }
    }
}
