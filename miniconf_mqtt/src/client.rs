use core::{convert::Infallible, marker::PhantomData};

use heapless::String;
use log::{error, info, warn};
use miniconf::{
    DescendError, Path, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize, ValueError,
    json_core,
};
use minimq::publication::ToPayload;
use minimq::{
    ConfigBuilder, Event, InboundPublish, ProtocolError, PubError, Publication, QoS, Session,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};

#[cfg(feature = "introspection")]
use crate::introspection::{json_text, state_info};
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

#[derive(Copy, Clone)]
enum Resource {
    Settings,
    Schema,
    State,
}

impl Resource {
    fn parse<'a>(topic: &'a str, prefix: &str) -> Option<(Self, &'a str)> {
        let tail = topic.strip_prefix(prefix)?;
        [
            (Self::Settings, "/settings"),
            (Self::Schema, "/schema"),
            (Self::State, "/state"),
        ]
        .into_iter()
        .find_map(|(resource, base)| tail.strip_prefix(base).map(|path| (resource, path)))
    }
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
        })
    }

    /// Construct a new MQTT settings interface.
    pub fn new(
        prefix: &'a str,
        connector: &'a C,
        config: ConfigBuilder<'a>,
    ) -> Result<Self, ProtocolError> {
        #[cfg(feature = "introspection")]
        // Introspection probes append one impossible descendant key to distinguish
        // present subtrees from `Absent`/`Access`, so callers need one slot beyond
        // the maximum valid key depth.
        const {
            assert!(Settings::SCHEMA.shape().max_depth < Y)
        }
        #[cfg(not(feature = "introspection"))]
        const {
            assert!(Settings::SCHEMA.shape().max_depth <= Y)
        }
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
    ///
    /// The dump is queued immediately and then published incrementally from later
    /// [`poll`](Self::poll) calls after the client has been
    /// [`activate`](Self::activate)d.
    pub fn dump(&mut self, path: Option<&str>) -> Result<(), Error> {
        if self.pending.is_active() {
            return Err(Error::Busy);
        }
        self.pending = Pending::dump(Settings::SCHEMA, path)?;
        Ok(())
    }

    /// Poll the MQTT service once.
    ///
    /// This is the primary driver entry point. Call it regularly in the main loop.
    /// `poll()` advances the shared MQTT session, processes inbound settings
    /// requests, ensures the service is activated on each fresh connection, and
    /// drains pending multipart replies and dumps.
    ///
    /// Normal usage is:
    /// - construct the client with [`new`](Self::new)
    /// - call `poll()` regularly
    /// - call [`publish`](Self::publish) for application messages when needed
    /// - call [`dump`](Self::dump) to enqueue a settings dump that later `poll()`
    ///   iterations will publish
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

        self.activate().await?;

        let changed = self.execute(settings, action).await;
        self.advance_pending(settings).await;
        Ok(changed)
    }

    /// Return whether the shared session is locally ready for a publish at the requested QoS.
    ///
    /// This is pessimistic for local backpressure: if it returns `false`,
    /// [`publish`](Self::publish) would currently fail because the shared MQTT
    /// session has no local publish capacity for that QoS.
    ///
    /// It is optimistic overall: if it returns `true`, `publish()` can still
    /// fail for serialization, packet-size, disconnect, or transport reasons.
    pub fn can_publish(&mut self, qos: QoS) -> bool {
        self.session.can_publish(qos)
    }

    /// Ensure the MQTT service is active on the current connection.
    ///
    /// Activation means the client is connected, has published the retained
    /// `"<prefix>/alive"` state for this connection, and has installed the
    /// `"<prefix>/settings/#"` subscription.
    ///
    /// You usually do not need to call this directly if you already call
    /// [`poll`](Self::poll) regularly, because `poll()` activates automatically.
    /// It is available for callers that want to force that setup before their
    /// first application publish.
    pub async fn activate(&mut self) -> Result<(), Error> {
        if self.needs_alive {
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
        let mut settings = topic.clone();
        settings
            .push_str("/settings/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let mut schema = topic.clone();
        schema
            .push_str("/schema/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let mut state = topic;
        state
            .push_str("/state/#")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let topics = [
            TopicFilter::new(&settings).options(opts),
            TopicFilter::new(&schema).options(opts),
            TopicFilter::new(&state).options(opts),
        ];
        self.session.subscribe(&topics, &[]).await?;
        self.subscribed = true;
        info!("Subscribed");
        Ok(())
    }

    /// Publish an application message on the shared MQTT session.
    ///
    /// This first [`activate`](Self::activate)s the service on the current
    /// connection and then publishes the provided message.
    pub async fn publish<P>(
        &mut self,
        publication: Publication<'_, P>,
    ) -> Result<(), PubError<P::Error>>
    where
        P: ToPayload,
    {
        self.activate().await.map_err(|err| match err {
            Error::Mqtt(err) => PubError::Session(err),
            Error::Busy | Error::Miniconf(_) => {
                unreachable!("activate path does not produce miniconf-specific errors")
            }
        })?;
        self.session.publish(publication).await
    }

    fn on_session_active(&mut self, reconnected: bool) {
        if !reconnected {
            self.subscribed = false;
        }
        self.needs_alive = true;
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
        let Some((resource, path)) = Resource::parse(message.topic, prefix)
            .map(|(resource, path)| (resource, Path::new(path, SEPARATOR)))
        else {
            return Action::None(State::Unchanged);
        };

        let has_response_topic = message.response_topic().is_some();
        let reply = match message.reply_owned() {
            Ok(reply) => reply,
            Err(err) => {
                warn!(
                    "Rejecting request with oversized reply target on {}: {err:?}",
                    message.topic
                );
                return Action::None(State::Unchanged);
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
                };
                return Action::ReplyText {
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message(err),
                };
            }
        };

        if matches!(resource, Resource::Schema | Resource::State) {
            #[cfg(not(feature = "introspection"))]
            {
                return if let Some(reply) = reply {
                    Action::ReplyText {
                        state: State::Unchanged,
                        reply: Some(reply),
                        code: ResponseCode::Error,
                        text: format_message("Introspection is disabled"),
                    }
                } else {
                    Action::None(State::Unchanged)
                };
            }
            #[cfg(feature = "introspection")]
            {
                if !message.payload.is_empty() {
                    return Action::ReplyText {
                        state: State::Unchanged,
                        reply,
                        code: ResponseCode::Error,
                        text: format_message("Schema/state endpoints are read-only"),
                    };
                }
                if reply.is_none() {
                    return Action::None(State::Unchanged);
                }
                let text = match resource {
                    Resource::Schema => json_text(&Settings::SCHEMA.get_node_info(path).unwrap()),
                    Resource::State => json_text(&state_info::<_, Y>(
                        settings,
                        &state[..lookup.depth],
                        lookup.schema,
                    )),
                    Resource::Settings => unreachable!(),
                };
                return match text {
                    Ok(text) => Action::ReplyText {
                        state: State::Unchanged,
                        reply,
                        code: ResponseCode::Ok,
                        text,
                    },
                    Err(()) => Action::ReplyText {
                        state: State::Unchanged,
                        reply,
                        code: ResponseCode::Error,
                        text: format_message("Response too long"),
                    },
                };
            }
        }

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
            } else if has_response_topic {
                Action::None(State::Unchanged)
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
                if let Some(reply) = &reply {
                    self.reply_text(reply, code, text.as_str()).await;
                }
                state
            }
            Action::ReplyLeaf {
                reply,
                state,
                depth,
            } => {
                if let Some(reply) = &reply {
                    self.reply_leaf(settings, reply, state, depth).await;
                }
                State::Unchanged
            }
            Action::SetPending { pending } => {
                self.pending = pending;
                State::Unchanged
            }
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
            info!("Response failure: {:?}", simple_pub_error(err));
        }
    }

    async fn reply_leaf(
        &mut self,
        settings: &Settings,
        reply: &ReplyTarget,
        state: [usize; Y],
        depth: usize,
    ) {
        let publication = reply.publication(|buf: &mut [u8]| {
            let full = &state[..depth];
            Self::with_leaf(full, |keys| json_core::get_by_keys(settings, keys, buf))
        });
        let props = [ResponseCode::Ok.into()];

        match self
            .session
            .publish(publication.properties(&props).qos(QoS::AtLeastOnce))
            .await
        {
            Ok(()) => {}
            Err(minimq::PubError::Payload(err)) => {
                self.reply_text(reply, ResponseCode::Error, format_message(err).as_str())
                    .await;
            }
            Err(minimq::PubError::Session(err)) => info!("Leaf response failure: {err:?}"),
        }
    }

    async fn advance_pending(&mut self, settings: &Settings) {
        while self.session.can_publish(QoS::AtLeastOnce) {
            match &mut self.pending {
                Pending::Idle => return,
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
                    let publication = reply
                        .publication(payload.as_bytes())
                        .properties(&props)
                        .qos(QoS::AtLeastOnce);
                    if let Err(err) = self.session.publish(publication).await {
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
                        Err(minimq::PubError::Payload(DepthError {
                            inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                            ..
                        })) => {}
                        Err(minimq::PubError::Payload(err)) => {
                            let props = [ResponseCode::Error.into()];
                            let text = format_message(err);
                            let publication = Publication::new(&topic, text.as_str())
                                .properties(&props)
                                .qos(QoS::AtLeastOnce);
                            if let Err(err) = self.session.publish(publication).await {
                                info!(
                                    "Multipart dump error response failure: {:?}",
                                    simple_pub_error(err)
                                );
                                self.pending.clear();
                                return;
                            }
                        }
                        Err(minimq::PubError::Session(err)) => {
                            info!("Multipart dump publish failure: {err:?}");
                            self.pending.clear();
                            return;
                        }
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
