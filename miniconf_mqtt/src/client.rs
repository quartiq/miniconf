use core::{convert::Infallible, marker::PhantomData};

use heapless::String;
use log::{error, info, warn};
use miniconf::{
    DescendError, Path, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize, ValueError,
    json_core,
};
use minimq::{
    ConfigBuilder, Event, InboundPublish, ProtocolError, Publication, QoS, Session,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};

use crate::{
    MAX_TOPIC_LENGTH, SEPARATOR,
    pending::Pending,
    protocol::{DepthError, ReplyTarget, ResponseCode, format_message, simple_pub_error},
};

/// Miniconf MQTT error.
#[derive(Debug, PartialEq, thiserror::Error)]
pub enum Error {
    /// A multipart operation is already in progress.
    #[error("a multipart operation is already in progress")]
    Busy,
    /// Miniconf path resolution failed.
    #[error("miniconf path resolution failed: {0}")]
    Miniconf(DescendError<()>),
    /// MQTT or transport operation failed.
    #[error(transparent)]
    Mqtt(#[from] minimq::Error),
}

impl From<DescendError<()>> for Error {
    fn from(value: DescendError<()>) -> Self {
        Self::Miniconf(value)
    }
}

pub(crate) enum Action<const Y: usize> {
    None(State),
    ReplyText {
        state: State,
        reply: Option<ReplyTarget>,
        code: ResponseCode,
        text: String<{ crate::MAX_RESPONSE_LENGTH }>,
    },
    ReplyLeaf {
        reply: Option<ReplyTarget>,
        state: [usize; Y],
        depth: usize,
    },
    SetPending {
        pending: Pending<Y>,
    },
}

/// Result of polling the MQTT service.
#[derive(Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    /// No setting changed.
    #[default]
    Unchanged,
    /// At least one setting changed.
    Changed,
}

/// Async MQTT settings interface.
pub struct MqttClient<'a, Settings, C, const Y: usize>
where
    C: Connector,
{
    session: Session<'a, 'a, C>,
    prefix: &'a str,
    alive: &'a str,
    subscribed: bool,
    needs_alive: bool,
    pending: Pending<Y>,
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
            leaf: Some(true),
        })
    }

    /// Construct a new MQTT settings interface.
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
        let will = minimq::OwnedWill::new(&will_topic, b"", &[])?
            .retained()
            .qos(QoS::AtMostOnce);
        let config = config.autodowngrade_qos().owned_will(will)?.build();

        Ok(Self {
            session: Session::new(config, connector),
            prefix,
            alive: "1",
            subscribed: false,
            needs_alive: true,
            pending: Pending::new(),
            _settings: PhantomData,
        })
    }

    /// Set the payload published on the `/alive` topic when connected.
    pub fn set_alive(&mut self, alive: &'a str) {
        self.alive = alive;
        self.needs_alive = true;
    }

    /// Schedule a dump of all leaf settings at or below `path`.
    pub fn dump(&mut self, path: Option<&str>) -> Result<(), Error> {
        if self.pending.is_active() {
            return Err(Error::Busy);
        }
        self.pending = Pending::dump(Settings::SCHEMA, path)?;
        Ok(())
    }

    /// Poll the MQTT service once.
    pub async fn poll(&mut self, settings: &mut Settings) -> Result<State, Error> {
        let pending_active = self.pending.is_active();
        let prefix = self.prefix;
        let (activation, action) = match self.session.poll().await? {
            Event::Connected => (Some(false), Action::None(State::Unchanged)),
            Event::Reconnected => (Some(true), Action::None(State::Unchanged)),
            Event::Idle => (None, Action::None(State::Unchanged)),
            Event::Inbound(message) => (
                None,
                Self::plan_request(prefix, pending_active, settings, &message),
            ),
        };

        if let Some(reconnected) = activation {
            self.on_session_active(reconnected);
        }

        self.ensure_ready().await?;

        let changed = self.execute(settings, action).await;
        self.advance_pending(settings).await;
        Ok(changed)
    }

    fn on_session_active(&mut self, reconnected: bool) {
        if !reconnected {
            self.subscribed = false;
        }
        self.needs_alive = true;
    }

    async fn ensure_ready(&mut self) -> Result<(), Error> {
        if self.needs_alive {
            self.publish_alive().await?;
            self.needs_alive = false;
        }
        if self.subscribed {
            return Ok(());
        }

        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/settings/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let topics = [TopicFilter::new(&topic).options(opts)];
        self.session.subscribe(&topics, &[]).await?;
        self.subscribed = true;
        info!("Subscribed");
        Ok(())
    }

    async fn publish_alive(&mut self) -> Result<(), Error> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/alive")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let publication = Publication::new(&topic, self.alive.as_bytes())
            .qos(QoS::AtLeastOnce)
            .retain();
        self.session
            .publish(publication)
            .await
            .map_err(simple_pub_error)
    }

    pub(crate) fn plan_request(
        prefix: &str,
        pending_active: bool,
        settings: &mut Settings,
        message: &InboundPublish<'_>,
    ) -> Action<Y> {
        let Some(path) = message
            .topic
            .strip_prefix(prefix)
            .and_then(|tail| tail.strip_prefix("/settings"))
            .map(|tail| Path::new(tail, SEPARATOR))
        else {
            return Action::None(State::Unchanged);
        };

        let reply = match message.reply_owned() {
            Ok(reply) => reply,
            Err(err) => {
                warn!(
                    "Ignoring oversized reply target on {}: {err:?}",
                    message.topic
                );
                None
            }
        };

        if pending_active {
            return Action::ReplyText {
                state: State::Unchanged,
                reply,
                code: ResponseCode::Error,
                text: format_message("Pending multipart response"),
            };
        }

        let mut state = [0; Y];
        let lookup = match Settings::SCHEMA.resolve_into(path, &mut state) {
            Ok(lookup) => lookup,
            Err(err) => {
                let err = DepthError::<Infallible> {
                    inner: match err.error {
                        DescendError::Key(err) => SerdeError::Value(ValueError::Key(err)),
                        DescendError::Inner(()) => {
                            SerdeError::Value(ValueError::Access("Insufficient state"))
                        }
                    },
                    depth: err.depth,
                    leaf: err.leaf,
                };
                return Action::ReplyText {
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message(err),
                };
            }
        };

        if message.payload.is_empty() {
            if lookup.schema.is_leaf() {
                Action::ReplyLeaf {
                    reply,
                    state,
                    depth: lookup.depth,
                }
            } else if let Some(reply) = reply {
                match Pending::list(Settings::SCHEMA, &state[..lookup.depth], reply) {
                    Ok(pending) => Action::SetPending { pending },
                    Err(err) => Action::ReplyText {
                        state: State::Unchanged,
                        reply: None,
                        code: ResponseCode::Error,
                        text: format_message(err),
                    },
                }
            } else {
                match Pending::dump_root(Settings::SCHEMA, &state[..lookup.depth]) {
                    Ok(pending) => Action::SetPending { pending },
                    Err(err) => {
                        info!("Dump scheduling failure: {err}");
                        Action::None(State::Unchanged)
                    }
                }
            }
        } else if !lookup.schema.is_leaf() {
            Action::ReplyText {
                state: State::Unchanged,
                reply,
                code: ResponseCode::Error,
                text: format_message("Path does not resolve to a leaf"),
            }
        } else {
            let full = &state[..lookup.depth];
            match Self::with_leaf(full, |keys| {
                json_core::set_by_keys(settings, keys, message.payload)
            }) {
                Ok(_) => Action::ReplyText {
                    state: State::Changed,
                    reply,
                    code: ResponseCode::Ok,
                    text: format_message("OK"),
                },
                Err(inner) => Action::ReplyText {
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message(inner),
                },
            }
        }
    }

    async fn execute(&mut self, settings: &Settings, action: Action<Y>) -> State {
        match action {
            Action::None(state) => state,
            Action::ReplyText {
                state,
                reply,
                code,
                text,
            } => {
                self.reply_text(reply.as_ref(), code, text.as_str()).await;
                state
            }
            Action::ReplyLeaf {
                reply,
                state,
                depth,
            } => {
                self.reply_leaf(settings, reply.as_ref(), state, depth)
                    .await;
                State::Unchanged
            }
            Action::SetPending { pending } => {
                self.pending = pending;
                State::Unchanged
            }
        }
    }

    async fn reply_text(&mut self, reply: Option<&ReplyTarget>, code: ResponseCode, text: &str) {
        let Some(publication) = reply.map(|target| target.publication(text.as_bytes())) else {
            return;
        };
        let props = [code.into()];
        if let Err(err) = self
            .session
            .publish(publication.properties(&props).qos(QoS::AtLeastOnce))
            .await
        {
            info!("Response failure: {:?}", simple_pub_error(err));
        }
    }

    async fn reply_leaf(
        &mut self,
        settings: &Settings,
        reply: Option<&ReplyTarget>,
        state: [usize; Y],
        depth: usize,
    ) {
        let Some(publication) = reply.map(|target| {
            target.publication(|buf: &mut [u8]| {
                let full = &state[..depth];
                Self::with_leaf(full, |keys| json_core::get_by_keys(settings, keys, buf))
            })
        }) else {
            return;
        };
        let props = [ResponseCode::Ok.into()];

        match self
            .session
            .publish(publication.properties(&props).qos(QoS::AtLeastOnce))
            .await
        {
            Ok(()) => {}
            Err(minimq::PubError::Serialization(err)) => {
                self.reply_text(reply, ResponseCode::Error, format_message(err).as_str())
                    .await;
            }
            Err(minimq::PubError::Error(err)) => info!("Leaf response failure: {err:?}"),
        }
    }

    async fn advance_pending(&mut self, settings: &Settings) {
        if !self.session.is_publish_ready(QoS::AtLeastOnce) {
            return;
        }

        match &mut self.pending {
            Pending::Idle => {}
            Pending::List { iter, reply } => {
                let (code, payload, done) = if let Some(path) = iter.next() {
                    let path = match path {
                        Ok(path) => path.into_inner(),
                        Err(err) => {
                            error!("Path iter error: {err}");
                            return;
                        }
                    };
                    (ResponseCode::Continue, path, false)
                } else {
                    (ResponseCode::Ok, String::new(), true)
                };

                let props = [code.into()];
                if let Err(err) = self
                    .session
                    .publish(
                        reply
                            .publication(payload.as_bytes())
                            .properties(&props)
                            .qos(QoS::AtLeastOnce),
                    )
                    .await
                {
                    info!(
                        "Multipart list publish failure: {:?}",
                        simple_pub_error(err)
                    );
                    self.pending.clear();
                    return;
                }
                if done {
                    self.pending.clear();
                }
            }
            Pending::Dump { .. } => {
                let Some((topic, state, depth)) = self.next_dump_step() else {
                    self.pending.clear();
                    return;
                };
                let props = [ResponseCode::Ok.into()];
                let publication = Publication::new(&topic, |buf: &mut [u8]| {
                    let full = &state[..depth];
                    Self::with_leaf(full, |keys| json_core::get_by_keys(settings, keys, buf))
                })
                .properties(&props)
                .qos(QoS::AtLeastOnce);

                match self.session.publish(publication).await {
                    Ok(()) => {}
                    Err(minimq::PubError::Serialization(DepthError {
                        inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                        ..
                    })) => {}
                    Err(minimq::PubError::Serialization(err)) => {
                        info!("Multipart dump serialization failure: {err}");
                        self.pending.clear();
                    }
                    Err(minimq::PubError::Error(err)) => {
                        info!("Multipart dump publish failure: {err:?}");
                        self.pending.clear();
                    }
                }
            }
        }
    }

    fn next_dump_step(&mut self) -> Option<(String<MAX_TOPIC_LENGTH>, [usize; Y], usize)> {
        let Pending::Dump { iter } = &mut self.pending else {
            return None;
        };

        loop {
            let path = match iter.next()? {
                Ok(path) => path.into_inner(),
                Err(err) => {
                    error!("Path iter error: {err}");
                    continue;
                }
            };
            let full = iter.state()?;
            let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().ok()?;
            topic.push_str("/settings").ok()?;
            topic.push_str(&path).ok()?;
            let mut state = [0; Y];
            state[..full.len()].copy_from_slice(full);
            return Some((topic, state, full.len()));
        }
    }
}
