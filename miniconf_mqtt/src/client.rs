use core::{convert::Infallible, fmt::Write as _, marker::PhantomData};

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Duration;
#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Instant;
#[cfg(feature = "compat-settings-ingress")]
use embassy_time::with_deadline;
use heapless::String;
use log::{debug, info, warn};
use miniconf::{
    DescendError, FromConfig, Path, SerdeError, Transcode, TreeDeserializeOwned, TreeSchema,
    TreeSerialize, ValueError, json_core,
};
use minimq::publication::ToPayload;
use minimq::{
    ConfigBuilder, Event, InboundPublish, ProtocolError, PubError, Publication, QoS, Session,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};
use serde::Serialize;
use yafnv::Fnv as _;

use crate::{
    MAX_PAYLOAD_LENGTH, MAX_TOPIC_LENGTH, SEPARATOR,
    pending::{Pending, SchemaIds, compact_schema_def},
    protocol::{
        DepthError, ReplyTarget, ResponseCode, format_message, rev_property, simple_pub_error,
    },
};

#[cfg(feature = "compat-settings-ingress")]
const SETTINGS_RECOVERY_QUIESCENCE: Duration = Duration::from_millis(100);

#[derive(Debug, PartialEq, thiserror::Error)]
/// MM2 MQTT client error.
pub enum ClientError<E> {
    /// Static path resolution failed before touching the value.
    #[error("miniconf path resolution failed: {0}")]
    Miniconf(DescendError<()>),
    /// MQTT session or publication failure.
    #[error(transparent)]
    Mqtt(#[from] minimq::Error<E>),
}

/// Backward-compatible alias for the MM2 client error.
pub type Error<E> = ClientError<E>;

impl<E> From<DescendError<()>> for ClientError<E> {
    fn from(value: DescendError<()>) -> Self {
        Self::Miniconf(value)
    }
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum Action<const Y: usize> {
    None(State),
    Reply {
        state: State,
        reply: Option<ReplyTarget>,
        code: ResponseCode,
        text: String<MAX_PAYLOAD_LENGTH>,
    },
    PublishSet {
        resource: Resource,
        reply: Option<ReplyTarget>,
        state: [usize; Y],
        depth: usize,
    },
    #[cfg(feature = "compat-settings-ingress")]
    OverrideSet {
        state: [usize; Y],
        depth: usize,
    },
}

#[derive(Copy, Clone)]
enum Activation {
    Connected,
    Reconnected,
}

struct PollStep<const Y: usize> {
    activation: Option<Activation>,
    action: Action<Y>,
    settings_ingress: bool,
    idle: bool,
}

#[derive(Copy, Clone)]
pub(crate) enum Resource {
    Set,
    #[cfg(feature = "compat-settings-ingress")]
    Settings,
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

impl Resource {
    fn parse<'a>(topic: &'a str, prefix: &str) -> Option<(Self, &'a str)> {
        let tail = topic.strip_prefix(prefix)?;
        #[cfg(feature = "compat-settings-ingress")]
        {
            [(Self::Settings, "/settings"), (Self::Set, "/set")]
                .into_iter()
                .find_map(|(resource, base)| tail.strip_prefix(base).map(|path| (resource, path)))
        }
        #[cfg(not(feature = "compat-settings-ingress"))]
        {
            tail.strip_prefix("/set").map(|path| (Self::Set, path))
        }
    }
}

#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
/// Whether a handled request changed device state.
pub enum State {
    /// The request was ignored or rejected before mutation.
    #[default]
    Unchanged,
    /// The request updated at least one leaf value.
    Changed,
}

#[derive(Serialize)]
struct Alive {
    boot_id: u32,
    schema_rev: u32,
    pages: usize,
}

/// MM2 MQTT session wrapper for one Miniconf tree.
///
/// `Y` is the path-state depth and should usually be `Settings::SCHEMA.shape().max_depth`.
pub struct MqttClient<'a, Settings, C, const Y: usize>
where
    C: Connector,
{
    session: Session<'a, 'a, C>,
    prefix: &'a str,
    subscribed: bool,
    needs_alive: bool,
    pending: Pending<Y>,
    alive: Alive,
    rev: u32,
    publish_alive_after_sync: bool,
    #[cfg(feature = "compat-settings-ingress")]
    settings_ingress: SettingsIngressPhase,
    _settings: PhantomData<Settings>,
}

impl<'a, Settings, C, const Y: usize> MqttClient<'a, Settings, C, Y>
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
        const { assert!(Settings::SCHEMA.shape().max_depth <= Y) }
        let shape = Settings::SCHEMA.shape();
        if prefix.len() + "/settings".len() + shape.max_length("/") > MAX_TOPIC_LENGTH {
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
            pending: Pending::new(),
            alive: Alive {
                boot_id: 0,
                schema_rev: 0,
                pages: 0,
            },
            rev: 0,
            publish_alive_after_sync: false,
            #[cfg(feature = "compat-settings-ingress")]
            settings_ingress: SettingsIngressPhase::Runtime,
            _settings: PhantomData,
        })
    }

    /// Progress MQTT I/O, requests, and background mirror publication work.
    pub async fn poll(&mut self, settings: &mut Settings) -> Result<State, Error<C::Error>> {
        let step = self.poll_step(settings).await?;

        if step.settings_ingress {
            self.note_settings_ingress();
        }

        if let Some(activation) = step.activation {
            self.on_session_active(activation);
        }

        self.activate().await?;
        self.finish_settings_recovery(step.idle);
        let changed = self.execute(settings, step.action).await;
        self.advance_pending(settings).await;
        Ok(changed)
    }

    async fn poll_step(&mut self, settings: &mut Settings) -> Result<PollStep<Y>, Error<C::Error>> {
        let prefix = self.prefix;
        Ok(match self.poll_session().await? {
            Event::Connected => PollStep {
                activation: Some(Activation::Connected),
                action: Action::None(State::Unchanged),
                settings_ingress: false,
                idle: false,
            },
            Event::Reconnected => PollStep {
                activation: Some(Activation::Reconnected),
                action: Action::None(State::Unchanged),
                settings_ingress: false,
                idle: false,
            },
            Event::Idle => PollStep {
                activation: None,
                action: Action::None(State::Unchanged),
                settings_ingress: false,
                idle: true,
            },
            Event::Inbound(message) => PollStep {
                activation: None,
                action: Self::plan_request(prefix, settings, &message),
                settings_ingress: Self::is_settings_ingress(prefix, &message),
                idle: false,
            },
        })
    }

    #[cfg(feature = "compat-settings-ingress")]
    async fn poll_session(&mut self) -> Result<Event<'_>, Error<C::Error>> {
        match self.settings_recovery_wait_deadline() {
            Some(deadline) => match with_deadline(deadline, self.session.poll()).await {
                Ok(event) => event.map_err(Into::into),
                Err(_) => Ok(Event::Idle),
            },
            None => self.session.poll().await.map_err(Into::into),
        }
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    async fn poll_session(&mut self) -> Result<Event<'_>, Error<C::Error>> {
        self.session.poll().await.map_err(Into::into)
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

    fn on_session_active(&mut self, activation: Activation) {
        if matches!(activation, Activation::Reconnected) {
            self.needs_alive = true;
            info!("Reconnected MM2 session");
            #[cfg(feature = "compat-settings-ingress")]
            {
                self.settings_ingress = SettingsIngressPhase::Runtime;
            }
            return;
        }

        self.subscribed = false;
        self.alive.boot_id = self.alive.boot_id.wrapping_add(1);
        self.rev = 0;
        self.alive.schema_rev = 0;
        self.alive.pages = 0;
        self.pending = Pending::schema(Settings::SCHEMA);
        self.publish_alive_after_sync = false;
        self.needs_alive = false;
        info!("Activated MM2 session boot_id={}", self.alive.boot_id);
        #[cfg(feature = "compat-settings-ingress")]
        {
            self.settings_ingress = SettingsIngressPhase::Recovering {
                seen: false,
                deadline: None,
            };
            debug!("Starting settings ingress recovery");
        }
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

    async fn publish_alive(&mut self) -> Result<(), Error<C::Error>> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/alive")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let body = json_text::<MAX_PAYLOAD_LENGTH, _>(&self.alive)
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let publication = Publication::new(&topic, body.as_str())
            .qos(QoS::AtLeastOnce)
            .retain();
        self.session
            .publish(publication)
            .await
            .map_err(simple_pub_error)
    }

    pub(crate) fn plan_request(
        prefix: &str,
        settings: &mut Settings,
        message: &InboundPublish<'_>,
    ) -> Action<Y> {
        let Some((resource, _)) = Resource::parse(message.topic(), prefix) else {
            return Action::None(State::Unchanged);
        };

        let reply = match resource {
            Resource::Set => match message
                .response_topic()
                .map(|topic| ReplyTarget::new(topic, message.correlation_data()))
                .transpose()
            {
                Ok(reply) => reply,
                Err(err) => {
                    warn!(
                        "Rejecting request with oversized reply target on {}: {err:?}",
                        message.topic()
                    );
                    return Action::None(State::Unchanged);
                }
            },
            #[cfg(feature = "compat-settings-ingress")]
            Resource::Settings => None,
        };

        Self::plan_publish(prefix, settings, message.topic(), message.payload(), reply)
    }

    pub(crate) fn plan_publish(
        prefix: &str,
        settings: &mut Settings,
        topic: &str,
        payload: &[u8],
        reply: Option<ReplyTarget>,
    ) -> Action<Y> {
        let Some((resource, path)) = Resource::parse(topic, prefix)
            .map(|(parsed, path)| (parsed, Path::new(path, SEPARATOR)))
        else {
            return Action::None(State::Unchanged);
        };

        let mut state = [0; Y];
        let lookup = match Settings::SCHEMA.resolve_into(path, &mut state) {
            Ok(lookup) => lookup,
            Err(err) => {
                if matches!(resource, Resource::Set) {
                    debug!("Rejecting set request topic={} err={err:?}", topic);
                    let err = DepthError::<Infallible> {
                        inner: match err.error {
                            DescendError::Key(err) => SerdeError::Value(ValueError::Key(err)),
                            DescendError::Inner(()) => {
                                SerdeError::Value(ValueError::Access("Insufficient state"))
                            }
                        },
                        depth: err.depth,
                    };
                    return Action::Reply {
                        state: State::Unchanged,
                        reply,
                        code: ResponseCode::Error,
                        text: format_message(err),
                    };
                }
                return Action::None(State::Unchanged);
            }
        };

        if payload.is_empty() {
            if matches!(resource, Resource::Set) {
                debug!("Ignoring empty set payload topic={topic}");
            }
            return match resource {
                Resource::Set => Action::None(State::Unchanged),
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings if lookup.schema.is_leaf() => Action::OverrideSet {
                    state,
                    depth: lookup.depth,
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::None(State::Unchanged),
            };
        }

        if !lookup.schema.is_leaf() {
            if matches!(resource, Resource::Set) {
                debug!("Rejecting non-leaf set request topic={topic}");
            }
            return match resource {
                Resource::Set => Action::Reply {
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message("Path does not resolve to a leaf"),
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::None(State::Unchanged),
            };
        }

        let full = &state[..lookup.depth];
        match Self::with_leaf(full, |keys| json_core::set_by_keys(settings, keys, payload)) {
            Ok(_) => Action::PublishSet {
                resource,
                reply,
                state,
                depth: lookup.depth,
            },
            Err(inner) => match resource {
                Resource::Set => Action::Reply {
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message(inner),
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::OverrideSet {
                    state,
                    depth: lookup.depth,
                },
            },
        }
    }

    async fn execute(&mut self, settings: &Settings, action: Action<Y>) -> State {
        match action {
            Action::None(state) => state,
            Action::Reply {
                state,
                reply,
                code,
                text,
            } => {
                if let Some(reply) = &reply {
                    self.reply_text(reply, code, text.as_str()).await;
                }
                state
            }
            Action::PublishSet {
                resource,
                reply,
                state,
                depth,
            } => {
                if matches!(resource, Resource::Set) {
                    if let Err(err) = self.publish_leaf(settings, state, depth).await {
                        if let Some(reply) = &reply {
                            self.reply_text(
                                reply,
                                ResponseCode::Error,
                                format_message(err).as_str(),
                            )
                            .await;
                        }
                        return State::Unchanged;
                    }
                    self.queue_settings_sync();
                    if let Some(reply) = &reply {
                        self.reply_text(reply, ResponseCode::Ok, "").await;
                    }
                    return State::Changed;
                }

                #[cfg(feature = "compat-settings-ingress")]
                match self.settings_ingress {
                    SettingsIngressPhase::Recovering { .. } => State::Changed,
                    SettingsIngressPhase::Runtime => {
                        if self.publish_current(settings, state, depth).await.is_err() {
                            return State::Unchanged;
                        }
                        self.queue_settings_sync();
                        State::Changed
                    }
                }
                #[cfg(not(feature = "compat-settings-ingress"))]
                unreachable!()
            }
            #[cfg(feature = "compat-settings-ingress")]
            Action::OverrideSet { state, depth } =>
            {
                #[cfg(feature = "compat-settings-ingress")]
                match self.settings_ingress {
                    SettingsIngressPhase::Recovering { .. } => State::Unchanged,
                    SettingsIngressPhase::Runtime => {
                        let _ = self.publish_current(settings, state, depth).await;
                        State::Unchanged
                    }
                }
            }
        }
    }

    fn queue_settings_sync(&mut self) {
        if matches!(self.pending, Pending::Idle) {
            debug!("Queued retained settings sync");
            self.pending = Pending::settings(Settings::SCHEMA);
        }
    }

    async fn reply_text(&mut self, reply: &ReplyTarget, code: ResponseCode, text: &str) {
        let props = [code.into()];
        if let Err(err) = self
            .session
            .publish(
                reply
                    .publication(text.as_bytes())
                    .properties(&props)
                    .qos(QoS::AtLeastOnce),
            )
            .await
        {
            warn!("Failed to publish reply: {:?}", simple_pub_error(err));
        }
    }

    async fn publish_leaf(
        &mut self,
        settings: &Settings,
        state: [usize; Y],
        depth: usize,
    ) -> Result<(), Error<C::Error>> {
        self.try_publish_leaf(settings, state, depth)
            .await
            .map_err(simple_pub_error)
    }

    async fn try_publish_leaf(
        &mut self,
        settings: &Settings,
        state: [usize; Y],
        depth: usize,
    ) -> Result<(), PubError<DepthError<serde_json_core::ser::Error>, C::Error>> {
        let topic = self
            .settings_topic(&state[..depth])
            .map_err(|err| match err {
                Error::Mqtt(err) => PubError::Session(err),
                Error::Miniconf(_) => unreachable!(),
            })?;
        self.rev = self.rev.wrapping_add(1);
        let mut rev = String::<16>::new();
        let props = [rev_property(&mut rev, self.rev)];
        let publication = Publication::new(&topic, |buf: &mut [u8]| {
            let full = &state[..depth];
            Self::with_leaf(full, |keys| json_core::get_by_keys(settings, keys, buf))
        })
        .properties(&props)
        .qos(QoS::AtLeastOnce)
        .retain();
        self.session.publish(publication).await
    }

    async fn clear_leaf(&mut self, topic: &str) -> Result<(), Error<C::Error>> {
        self.rev = self.rev.wrapping_add(1);
        let mut rev = String::<16>::new();
        let props = [rev_property(&mut rev, self.rev)];
        let publication = Publication::new(topic, b"")
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        self.session
            .publish(publication)
            .await
            .map_err(simple_pub_error)
    }

    fn settings_topic(&self, state: &[usize]) -> Result<String<MAX_TOPIC_LENGTH>, Error<C::Error>> {
        let mut path = Path::<String<MAX_TOPIC_LENGTH>>::from_config(&SEPARATOR);
        path.transcode_from(Settings::SCHEMA, state)
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

    #[cfg(feature = "compat-settings-ingress")]
    async fn publish_current(
        &mut self,
        settings: &Settings,
        state: [usize; Y],
        depth: usize,
    ) -> Result<(), Error<C::Error>> {
        let topic = self.settings_topic(&state[..depth])?;
        match self.try_publish_leaf(settings, state, depth).await {
            Ok(()) => Ok(()),
            Err(PubError::Payload(DepthError {
                inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                ..
            })) => self.clear_leaf(&topic).await,
            Err(err) => Err(simple_pub_error(err)),
        }
    }

    #[cfg(feature = "compat-settings-ingress")]
    fn settings_recovery_wait_deadline(&self) -> Option<Instant> {
        match self.settings_ingress {
            SettingsIngressPhase::Recovering {
                seen: true,
                deadline: Some(deadline),
            } if matches!(self.pending, Pending::Idle) => Some(deadline),
            _ => None,
        }
    }

    #[cfg(feature = "compat-settings-ingress")]
    fn note_settings_ingress(&mut self) {
        if let SettingsIngressPhase::Recovering { seen, .. } = self.settings_ingress {
            if !seen {
                debug!("Observed retained settings ingress during recovery");
            }
            self.settings_ingress = SettingsIngressPhase::Recovering {
                seen: true,
                deadline: Some(Instant::now() + SETTINGS_RECOVERY_QUIESCENCE),
            };
        }
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    fn note_settings_ingress(&mut self) {}

    #[cfg(feature = "compat-settings-ingress")]
    fn finish_settings_recovery(&mut self, idle: bool) {
        let SettingsIngressPhase::Recovering {
            seen: true,
            deadline: Some(deadline),
        } = self.settings_ingress
        else {
            return;
        };
        if !idle || Instant::now() < deadline || !matches!(self.pending, Pending::Idle) {
            return;
        }
        self.settings_ingress = SettingsIngressPhase::Runtime;
        debug!("Finished settings ingress recovery");
        self.pending = Pending::settings(Settings::SCHEMA);
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    fn finish_settings_recovery(&mut self, _idle: bool) {}

    async fn advance_pending(&mut self, settings: &Settings) {
        if !self.session.can_publish(QoS::AtLeastOnce) {
            return;
        }
        match &mut self.pending {
            Pending::Idle => {}
            Pending::Schema { .. } => self.advance_schema_pending().await,
            Pending::Settings { .. } => self.advance_settings_pending(settings).await,
        }
    }

    async fn advance_schema_pending(&mut self) {
        let (finished, publish) = {
            let Pending::Schema {
                iter,
                ids,
                page,
                hash,
                carry,
            } = &mut self.pending
            else {
                unreachable!()
            };
            match next_schema_page(iter, ids, carry) {
                None => ((Some((*page, *hash))), None),
                Some(payload) => {
                    *hash = hash.fnv1a(payload.as_bytes().iter().copied());
                    let current_page = *page;
                    *page += 1;
                    (None, Some((current_page, payload)))
                }
            }
        };
        if let Some((pages, hash)) = finished {
            self.finish_schema_sync(pages, hash);
            return;
        }
        let Some((current_page, payload)) = publish else {
            unreachable!()
        };
        let topic = self.schema_page_topic(current_page);
        let publication = Publication::new(&topic, payload.as_str())
            .qos(QoS::AtLeastOnce)
            .retain();
        if let Err(err) = self.session.publish(publication).await {
            warn!(
                "Failed to publish schema page {}: {:?}",
                current_page,
                simple_pub_error(err)
            );
            self.pending.clear();
        }
    }

    fn finish_schema_sync(&mut self, pages: usize, hash: u32) {
        self.alive.pages = pages;
        self.alive.schema_rev = hash;
        self.publish_alive_after_sync = true;
        info!(
            "Completed schema sync pages={} rev={}",
            self.alive.pages, self.alive.schema_rev
        );
        #[cfg(feature = "compat-settings-ingress")]
        if matches!(
            self.settings_ingress,
            SettingsIngressPhase::Recovering { seen: true, .. }
        ) {
            debug!("Deferring retained settings sync until recovery completes");
            self.pending.clear();
            return;
        }
        debug!("Queued retained settings sync after schema sync");
        self.pending = Pending::settings(Settings::SCHEMA);
    }

    async fn advance_settings_pending(&mut self, settings: &Settings) {
        let (path, state, depth) = {
            let Pending::Settings { iter } = &mut self.pending else {
                unreachable!()
            };
            let Some(path) = iter.next() else {
                self.finish_settings_sync().await;
                return;
            };
            let path = match path {
                Ok(path) => path.into_inner(),
                Err(err) => {
                    warn!("Aborting retained settings sync after path iteration failure: {err}");
                    self.publish_alive_after_sync = false;
                    self.pending.clear();
                    return;
                }
            };
            let Some(full) = iter.state() else {
                self.pending.clear();
                return;
            };
            let mut state = [0; Y];
            state[..full.len()].copy_from_slice(full);
            (path, state, full.len())
        };

        let topic = self.settings_sync_topic(&path);
        match self.try_publish_leaf(settings, state, depth).await {
            Ok(()) => {}
            Err(PubError::Payload(DepthError {
                inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                ..
            })) => {
                if let Err(err) = self.clear_leaf(&topic).await {
                    warn!("Failed to clear retained setting path={path}: {err:?}");
                    self.publish_alive_after_sync = false;
                    self.pending.clear();
                }
            }
            Err(err) => {
                warn!(
                    "Failed to publish retained setting path={path}: {:?}",
                    simple_pub_error(err)
                );
                self.publish_alive_after_sync = false;
                self.pending.clear();
            }
        }
    }

    async fn finish_settings_sync(&mut self) {
        if self.publish_alive_after_sync {
            self.publish_alive_after_sync = false;
            if let Err(err) = self.publish_alive().await {
                warn!("Failed to publish alive manifest: {err:?}");
            } else {
                info!(
                    "Completed retained settings sync pages={} rev={}",
                    self.alive.pages, self.alive.schema_rev
                );
            }
        }
        self.pending.clear();
    }

    fn schema_page_topic(&self, page: usize) -> String<MAX_TOPIC_LENGTH> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        topic.push_str("/schema/").ok();
        write!(&mut topic, "{page}").ok();
        topic
    }

    fn settings_sync_topic(&self, path: &str) -> String<MAX_TOPIC_LENGTH> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        topic.push_str("/settings").ok();
        topic.push_str(path).ok();
        topic
    }
}

fn json_text<const N: usize, T: Serialize>(value: &T) -> Result<String<N>, ()> {
    let mut buf = [0u8; N];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    value.serialize(&mut ser).map_err(|_| ())?;
    let len = ser.end();
    let text = core::str::from_utf8(&buf[..len]).map_err(|_| ())?;
    let mut out = String::new();
    out.push_str(text).map_err(|_| ())?;
    Ok(out)
}

fn next_schema_page<const Y: usize>(
    iter: &mut miniconf::SchemaIter<Y>,
    ids: &SchemaIds<Y>,
    carry: &mut Option<String<MAX_PAYLOAD_LENGTH>>,
) -> Option<String<MAX_PAYLOAD_LENGTH>> {
    let mut payload = String::<MAX_PAYLOAD_LENGTH>::new();
    if let Some(line) = carry.take() {
        payload.push_str(&line).ok()?;
        payload.push('\n').ok()?;
    }
    for entry in iter.by_ref() {
        if !ids.is_first_occurrence(entry.schema(), &entry.state(), entry.depth()) {
            continue;
        }
        let id = ids.id_of(entry.schema());
        let Ok(line) = json_text::<MAX_PAYLOAD_LENGTH, _>(&compact_schema_def(entry.schema(), ids))
        else {
            warn!("Skipping oversized schema entry for definition {id}");
            continue;
        };
        let need = line.len() + 1;
        if !payload.is_empty() && payload.len() + need > MAX_PAYLOAD_LENGTH {
            *carry = Some(line);
            break;
        }
        if payload.push_str(&line).is_err() || payload.push('\n').is_err() {
            *carry = Some(line);
            break;
        }
    }
    (!payload.is_empty()).then_some(payload)
}
