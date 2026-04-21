mod request;
mod sync;

use core::marker::PhantomData;

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::{Instant, with_deadline};
use heapless::String;
use log::{debug, info};
use miniconf::{
    DescendError, IntoKeys, KeyError, NodeIter, SerdeError, TreeDeserializeOwned, TreeSchema,
    TreeSerialize,
};
use minimq::{
    ConfigBuilder, Event as SessionEvent, InboundPublish, Property, ProtocolError, PubError,
    Publication, QoS, Session,
    publication::ToPayload,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};

#[cfg(feature = "compat-settings-ingress")]
use crate::message::Resource;
use crate::{
    MAX_TOPIC_LENGTH,
    message::{Action, DepthError},
    schema::{SchemaDefs, SchemaSync, SettingsSync},
};

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
    Recovering {
        seen: bool,
        deadline: Option<Instant>,
    },
    Runtime,
}

#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) enum Change {
    #[default]
    Unchanged,
    Changed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
/// One app-visible outcome from one [`MqttClient::poll`] step.
pub enum Event {
    /// No app-visible event occurred.
    Idle,
    /// An MM2 request updated at least one setting leaf.
    Changed,
    /// The broker created a fresh MQTT session.
    Connected,
    /// The broker resumed the existing MQTT session.
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
    phase: Phase,
    followup: Followup,
    #[cfg(feature = "compat-settings-ingress")]
    settings_ingress: SettingsIngressPhase,
}

#[derive(Default)]
struct Followup {
    publish_alive: bool,
    publish_all: bool,
}

#[allow(clippy::large_enum_variant)]
enum Phase {
    Schema(SchemaSync),
    Settings(SettingsSync),
    Idle,
}

impl ProtocolState {
    fn new() -> Self {
        Self {
            manifest: Manifest::default(),
            phase: Phase::Idle,
            followup: Followup::default(),
            #[cfg(feature = "compat-settings-ingress")]
            settings_ingress: SettingsIngressPhase::Runtime,
        }
    }

    fn on_session_active<Settings: TreeSchema>(&mut self, reconnected: bool) {
        if reconnected {
            info!("Reconnected MM2 session");
            #[cfg(feature = "compat-settings-ingress")]
            {
                self.settings_ingress = SettingsIngressPhase::Runtime;
            }
            return;
        }

        self.manifest.epoch = self.manifest.epoch.wrapping_add(1);
        self.manifest.settings_rev = 0;
        self.manifest.schema_rev = 0;
        self.manifest.schema_pages = 0;
        self.phase = Phase::Schema(SchemaSync::new(Settings::SCHEMA));
        self.followup = Followup::default();
        info!("Activated MM2 session epoch={}", self.manifest.epoch);
        #[cfg(feature = "compat-settings-ingress")]
        {
            self.settings_ingress = SettingsIngressPhase::Recovering {
                seen: false,
                deadline: None,
            };
            debug!("Starting settings ingress recovery");
        }
    }
}

/// MM2 MQTT session wrapper for one Miniconf tree.
pub struct MqttClient<'a, Settings, C>
where
    C: Connector,
{
    session: Session<'a, 'a, C>,
    prefix: &'a str,
    subscribed: bool,
    needs_alive: bool,
    protocol: ProtocolState,
    _settings: PhantomData<Settings>,
}

impl<'a, Settings, C> MqttClient<'a, Settings, C>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    C: Connector,
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
    pub fn new(
        prefix: &'a str,
        connector: &'a C,
        config: ConfigBuilder<'a>,
    ) -> Result<Self, ProtocolError> {
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
            session: Session::new(config, connector),
            prefix,
            subscribed: false,
            needs_alive: true,
            protocol: ProtocolState::new(),
            _settings: PhantomData,
        })
    }

    /// Progress MQTT I/O, requests, and background mirror publication work.
    ///
    /// Background schema and retained-settings sync is incremental: each `poll()` handles one
    /// session event first and then usually advances at most one retained publication step.
    /// Pending sync work therefore does not spin in an internal loop or monopolize the networking
    /// stack, but it can still consume publish capacity across successive polls until it
    /// completes.
    ///
    /// Non-MM2 inbound publishes are delivered to `on_other`.
    ///
    /// Each `poll()` still handles one session event first and then advances at most one MM2
    /// background step.
    pub async fn poll(
        &mut self,
        settings: &mut Settings,
        mut on_other: impl FnMut(&InboundPublish<'_>),
    ) -> Result<Event, Error<C::Error>> {
        #[allow(clippy::large_enum_variant)]
        enum Step {
            Connected,
            Reconnected,
            Idle,
            Handled {
                settings_ingress: bool,
                action: Action,
            },
            Other,
        }

        let prefix = self.prefix;
        let step = match {
            #[cfg(feature = "compat-settings-ingress")]
            {
                match self.settings_recovery_wait_deadline() {
                    Some(deadline) => match with_deadline(deadline, self.session.poll()).await {
                        Ok(event) => event.map_err(Error::from),
                        Err(_) => Ok(SessionEvent::Idle),
                    },
                    None => self.session.poll().await.map_err(Error::from),
                }
            }
            #[cfg(not(feature = "compat-settings-ingress"))]
            {
                self.session.poll().await.map_err(Error::from)
            }
        }? {
            SessionEvent::Connected => Step::Connected,
            SessionEvent::Reconnected => Step::Reconnected,
            SessionEvent::Idle => Step::Idle,
            SessionEvent::Inbound(message) => {
                let settings_ingress = Self::is_settings_ingress(prefix, &message);
                let action = Self::plan_request(prefix, settings, &message);
                if matches!(action, Action::Unhandled) {
                    on_other(&message);
                    Step::Other
                } else {
                    Step::Handled {
                        settings_ingress,
                        action,
                    }
                }
            }
        };

        match step {
            Step::Other => {
                self.activate().await?;
                self.finish_settings_recovery(false);
                self.advance_pending(settings).await;
                Ok(Event::Other)
            }
            Step::Connected => {
                self.on_session_active(false);
                self.activate().await?;
                self.finish_settings_recovery(false);
                self.advance_pending(settings).await;
                Ok(Event::Connected)
            }
            Step::Reconnected => {
                self.on_session_active(true);
                self.activate().await?;
                self.finish_settings_recovery(false);
                self.advance_pending(settings).await;
                Ok(Event::Reconnected)
            }
            Step::Idle => {
                self.activate().await?;
                self.finish_settings_recovery(true);
                self.advance_pending(settings).await;
                Ok(Event::Idle)
            }
            Step::Handled {
                settings_ingress,
                action,
            } => {
                if settings_ingress {
                    self.note_settings_ingress();
                }
                let outcome = match self.execute(settings, action).await {
                    Change::Unchanged => Event::Idle,
                    Change::Changed => Event::Changed,
                };
                self.activate().await?;
                self.finish_settings_recovery(false);
                self.advance_pending(settings).await;
                Ok(outcome)
            }
        }
    }

    /// Publish one retained leaf value by exact key.
    ///
    /// This is the efficient app-side hook for a known leaf change. If the key resolves to an
    /// internal node, use [`publish_all`](Self::publish_all) after the structural change instead.
    pub async fn publish_by_key(
        &mut self,
        settings: &Settings,
        key: impl IntoKeys,
    ) -> Result<(), Error<C::Error>> {
        let mut state = [0; crate::MAX_DEPTH];
        let lookup = Settings::SCHEMA
            .resolve_into(key, &mut state)
            .map_err(|err| err.error)?;
        if !lookup.schema.is_leaf() {
            return Err(Error::Miniconf(DescendError::Key(KeyError::TooShort)));
        }
        self.activate().await?;
        self.publish_current(settings, state, lookup.depth).await
    }

    /// Queue a background retained full-tree republish.
    ///
    /// The queued sync is advanced by [`poll`](Self::poll).
    pub fn publish_all(&mut self) {
        self.protocol.followup.publish_all = true;
        self.start_settings_sync();
    }

    /// Subscribe additional non-MM2 topics on the shared session.
    ///
    /// The caller owns these subscriptions. Re-subscribe after [`Event::Connected`].
    pub async fn subscribe(
        &mut self,
        topics: &[TopicFilter<'_>],
        properties: &[Property<'_>],
    ) -> Result<(), Error<C::Error>> {
        self.activate().await?;
        self.session.subscribe(topics, properties).await?;
        Ok(())
    }

    /// Unsubscribe additional non-MM2 topics from the shared session.
    pub async fn unsubscribe(
        &mut self,
        topics: &[&str],
        properties: &[Property<'_>],
    ) -> Result<(), Error<C::Error>> {
        self.activate().await?;
        self.session.unsubscribe(topics, properties).await?;
        Ok(())
    }

    /// Whether the MQTT session can currently publish at the requested QoS.
    pub fn can_publish(&mut self, qos: QoS) -> bool {
        self.session.can_publish(qos)
    }

    /// Ensure retained manifest publication and ingress subscriptions are active.
    pub async fn activate(&mut self) -> Result<(), Error<C::Error>> {
        if self.needs_alive {
            debug!("Publishing alive manifest");
            self.publish_alive().await?;
            self.needs_alive = false;
        }
        if self.subscribed {
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
        #[cfg(feature = "compat-settings-ingress")]
        let mut compat = topic.clone();
        #[cfg(feature = "compat-settings-ingress")]
        compat
            .push_str("/settings/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let topics = {
            #[cfg(feature = "compat-settings-ingress")]
            {
                [
                    TopicFilter::new(&set).options(opts),
                    TopicFilter::new(&compat).options(opts),
                ]
            }
            #[cfg(not(feature = "compat-settings-ingress"))]
            {
                [TopicFilter::new(&set).options(opts)]
            }
        };
        self.session.subscribe(&topics, &[]).await?;
        self.subscribed = true;
        debug!("Subscribed request topics");
        Ok(())
    }

    /// Publish an arbitrary MQTT packet after MM2 activation.
    pub async fn publish<P>(
        &mut self,
        publication: Publication<'_, P>,
    ) -> Result<(), PubError<P::Error, C::Error>>
    where
        P: ToPayload,
    {
        self.activate().await.map_err(|err| match err {
            Error::Mqtt(err) => PubError::Session(err),
            Error::Miniconf(_) => unreachable!(),
        })?;
        self.session.publish(publication).await
    }

    fn on_session_active(&mut self, reconnected: bool) {
        if reconnected {
            self.needs_alive = true;
            self.protocol.on_session_active::<Settings>(true);
            return;
        }

        self.subscribed = false;
        self.needs_alive = false;
        self.protocol.on_session_active::<Settings>(false);
    }

    #[cfg(feature = "compat-settings-ingress")]
    fn is_settings_ingress(prefix: &str, message: &InboundPublish<'_>) -> bool {
        matches!(
            Resource::parse(message.topic(), prefix),
            Some((Resource::Settings, _))
        )
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    fn is_settings_ingress(_prefix: &str, _message: &InboundPublish<'_>) -> bool {
        false
    }

    #[cfg(feature = "compat-settings-ingress")]
    fn can_start_settings_sync(&self) -> bool {
        matches!(
            self.protocol.settings_ingress,
            SettingsIngressPhase::Runtime
        )
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    fn can_start_settings_sync(&self) -> bool {
        true
    }

    fn start_settings_sync(&mut self) {
        if !self.can_start_settings_sync()
            || !matches!(self.protocol.phase, Phase::Idle)
            || !self.protocol.followup.publish_all
        {
            return;
        }
        debug!("Queued retained settings sync");
        self.protocol.phase = Phase::Settings(NodeIter::new(Settings::SCHEMA));
        self.protocol.followup.publish_all = false;
    }
}
