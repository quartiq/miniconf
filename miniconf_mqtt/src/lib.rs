#![no_std]
#![warn(missing_docs)]

//! Async MQTT interface for `miniconf`.

use core::{
    convert::Infallible,
    fmt::{Display, Write as FmtWrite},
    marker::PhantomData,
};

use heapless::{String, Vec};
use log::{error, info, warn};
use miniconf::{
    DescendError, IntoKeys, Lookup, NodeIter, Path, Schema, SerdeError, TreeDeserializeOwned,
    TreeSchema, TreeSerialize, ValueError, json_core,
};
pub use minimq;
use minimq::{
    ConfigBuilder, InboundPublish, ProtocolError, Publication, QoS, Runner, RunnerError,
    RunnerPubError,
    timer::Timer,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};
use strum::IntoStaticStr;

const MAX_TOPIC_LENGTH: usize = 128;
const MAX_RESPONSE_LENGTH: usize = 128;
const MAX_CD_LENGTH: usize = 32;
const SEPARATOR: char = '/';

/// Miniconf MQTT error.
#[derive(Debug, PartialEq)]
pub enum Error<Connect, Io, Time> {
    /// A multipart operation is already in progress.
    Busy,
    /// Miniconf path resolution failed.
    Miniconf(DescendError<()>),
    /// Establishing a TCP connection failed.
    Connect(Connect),
    /// MQTT or transport I/O failed.
    Mqtt(minimq::Error<Io>),
    /// Timer access failed.
    Timer(Time),
    /// `minimq::Runner` entered an invalid internal state.
    State,
}

impl<Connect, Io, Time> From<DescendError<()>> for Error<Connect, Io, Time> {
    fn from(value: DescendError<()>) -> Self {
        Self::Miniconf(value)
    }
}

impl<Connect, Io, Time> From<RunnerError<Connect, Io, Time>> for Error<Connect, Io, Time> {
    fn from(value: RunnerError<Connect, Io, Time>) -> Self {
        match value {
            RunnerError::Connect(err) => Self::Connect(err),
            RunnerError::Network(err) => Self::Mqtt(err),
            RunnerError::Timer(err) => Self::Timer(err),
            RunnerError::State => Self::State,
        }
    }
}

impl<Connect, Io, Time> From<RunnerPubError<Connect, Io, Time, Infallible>>
    for Error<Connect, Io, Time>
{
    fn from(value: RunnerPubError<Connect, Io, Time, Infallible>) -> Self {
        match value {
            RunnerPubError::Publish(err) => match err {
                minimq::PubError::Error(err) => Self::Mqtt(err),
                minimq::PubError::Serialization(err) => match err {},
            },
            RunnerPubError::Runner(err) => err.into(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, IntoStaticStr)]
enum ResponseCode {
    Ok,
    Continue,
    Error,
}

impl From<ResponseCode> for minimq::Property<'static> {
    fn from(value: ResponseCode) -> Self {
        minimq::Property::UserProperty(
            minimq::types::Utf8String("code"),
            minimq::types::Utf8String(value.into()),
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DepthError<E> {
    inner: SerdeError<E>,
    depth: usize,
    leaf: Option<bool>,
}

impl<E> Display for DepthError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.leaf {
            Some(leaf) => write!(f, "{} (depth {}, leaf {})", self.inner, self.depth, leaf),
            None => write!(f, "{} (depth {})", self.inner, self.depth),
        }
    }
}

#[derive(Clone)]
struct Request {
    topic: String<MAX_TOPIC_LENGTH>,
    response_topic: Option<String<MAX_TOPIC_LENGTH>>,
    correlation_data: Option<Vec<u8, MAX_CD_LENGTH>>,
}

impl Request {
    fn parse(message: &InboundPublish<'_>) -> Result<Self, &'static str> {
        let topic = String::try_from(message.topic).map_err(|_| "Topic too long")?;
        let response_topic = (&message.properties)
            .into_iter()
            .response_topic()
            .map(TryInto::try_into)
            .transpose()
            .map_err(|_| "Response topic too long")?;
        let correlation_data = (&message.properties)
            .into_iter()
            .find_map(|prop| {
                if let Ok(minimq::Property::CorrelationData(cd)) = prop {
                    Some(Vec::try_from(cd.0))
                } else {
                    None
                }
            })
            .transpose()
            .map_err(|_| "Correlation data too long")?;
        Ok(Self {
            topic,
            response_topic,
            correlation_data,
        })
    }

    fn reply<P>(&self, payload: P) -> Publication<'_, P> {
        let mut publication = Publication::new(
            self.response_topic
                .as_deref()
                .unwrap_or(self.topic.as_str()),
            payload,
        );
        if let Some(correlation_data) = &self.correlation_data {
            publication = publication.correlate(correlation_data);
        }
        publication
    }

    fn list_topic(&self) -> Result<&str, &'static str> {
        self.response_topic
            .as_deref()
            .ok_or("Internal node listing requires response topic")
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum PendingKind {
    Idle,
    List,
    Dump,
}

struct Pending<const Y: usize> {
    kind: PendingKind,
    iter: NodeIter<Path<String<MAX_TOPIC_LENGTH>>, Y>,
    request: Option<Request>,
}

impl<const Y: usize> Pending<Y> {
    fn new(schema: &'static Schema) -> Self {
        Self {
            kind: PendingKind::Idle,
            iter: NodeIter::new(schema, [0; Y], 0, SEPARATOR),
            request: None,
        }
    }

    fn is_active(&self) -> bool {
        self.kind != PendingKind::Idle
    }

    fn clear(&mut self) {
        self.kind = PendingKind::Idle;
        self.request = None;
    }

    fn dump(schema: &'static Schema, path: Option<&str>) -> Result<Self, DescendError<()>> {
        let iter = match path {
            Some(path) => NodeIter::with_root(schema, Path::new(path, SEPARATOR), SEPARATOR)?,
            None => NodeIter::new(schema, [0; Y], 0, SEPARATOR),
        };
        Ok(Self {
            kind: PendingKind::Dump,
            iter,
            request: None,
        })
    }

    fn list(
        schema: &'static Schema,
        root: &[usize],
        request: Request,
    ) -> Result<Self, &'static str> {
        request.list_topic()?;
        let iter = NodeIter::with_root(schema, root, SEPARATOR).map_err(|_| "Invalid list root")?;
        Ok(Self {
            kind: PendingKind::List,
            iter,
            request: Some(request),
        })
    }
}

enum Action<const Y: usize> {
    None(State),
    ReplyText {
        state: State,
        request: Request,
        code: ResponseCode,
        text: String<MAX_RESPONSE_LENGTH>,
    },
    ReplyLeaf {
        request: Request,
        state: [usize; Y],
        depth: usize,
        leaf: bool,
    },
    StartList {
        request: Request,
        state: [usize; Y],
        depth: usize,
    },
}

fn format_message<T: Display>(value: T) -> String<MAX_RESPONSE_LENGTH> {
    let mut text = String::new();
    if write!(&mut text, "{value}").is_err() {
        text.clear();
        text.push_str("Response too long").ok();
    }
    text
}

fn resolve<T, E, const Y: usize>(
    schema: &'static Schema,
    keys: impl IntoKeys,
    func: impl FnOnce(&mut &[usize], Lookup) -> Result<T, SerdeError<E>>,
) -> Result<T, DepthError<E>> {
    let mut state = [0; Y];
    let info = schema
        .resolve_into(keys, &mut state)
        .map_err(|err| DepthError {
            inner: match err.error {
                DescendError::Key(err) => SerdeError::Value(ValueError::Key(err)),
                DescendError::Inner(()) => {
                    SerdeError::Value(ValueError::Access("Insufficient state"))
                }
            },
            depth: err.depth,
            leaf: err.leaf,
        })?;
    let full = &state[..info.depth];
    let mut rest = full;
    func(&mut rest, info).map_err(|inner| DepthError {
        inner,
        depth: full.len() - rest.len(),
        leaf: Some(info.leaf),
    })
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

type ClientError<C, T> =
    Error<<C as Connector>::ConnectError, <C as Connector>::IoError, <T as Timer>::Error>;

/// Async MQTT settings interface.
pub struct MqttClient<'a, Settings, C, T, const Y: usize>
where
    C: Connector,
    T: Timer,
{
    mqtt: minimq::MqttClient<'a>,
    connector: &'a C,
    connection: Option<C::Connection<'a>>,
    timer: T,
    prefix: &'a str,
    alive: &'a str,
    subscribed: bool,
    needs_alive: bool,
    needs_initial_dump: bool,
    pending: Pending<Y>,
    _settings: PhantomData<Settings>,
}

impl<'a, Settings, C, T, const Y: usize> MqttClient<'a, Settings, C, T, Y>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    C: Connector,
    T: Timer,
{
    /// Construct a new MQTT settings interface.
    pub fn new(
        prefix: &'a str,
        connector: &'a C,
        timer: T,
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
        let will = minimq::Will::new_owned(&will_topic, b"", &[])?
            .retained()
            .qos(QoS::AtMostOnce);
        let config = config.autodowngrade_qos().will(will)?.build();

        Ok(Self {
            mqtt: minimq::MqttClient::new(config),
            connector,
            connection: None,
            timer,
            prefix,
            alive: "1",
            subscribed: false,
            needs_alive: true,
            needs_initial_dump: false,
            pending: Pending::new(Settings::SCHEMA),
            _settings: PhantomData,
        })
    }

    /// Set the payload published on the `/alive` topic when connected.
    pub fn set_alive(&mut self, alive: &'a str) {
        self.alive = alive;
        self.needs_alive = true;
    }

    /// Schedule a dump of all leaf settings at or below `path`.
    pub fn dump(&mut self, path: Option<&str>) -> Result<(), ClientError<C, T>> {
        if self.pending.is_active() {
            return Err(Error::Busy);
        }
        self.pending = Pending::dump(Settings::SCHEMA, path)?;
        Ok(())
    }

    /// Poll the MQTT service once.
    pub async fn poll(&mut self, settings: &mut Settings) -> Result<State, ClientError<C, T>> {
        let prefix = self.prefix;
        let pending_active = self.pending.is_active();
        let (reconnected, action) = {
            let mut runner = Runner::new(
                &mut self.mqtt,
                self.connector,
                &mut self.timer,
                &mut self.connection,
            );
            let outcome = runner.poll().await?;
            let action = outcome
                .inbound
                .as_ref()
                .map(|message| Self::plan_request(prefix, pending_active, settings, message))
                .unwrap_or(Action::None(State::Unchanged));
            (outcome.reconnected, action)
        };

        self.on_reconnected(reconnected);
        self.ensure_subscription().await?;

        if self.needs_initial_dump && !self.pending.is_active() {
            self.dump(None)?;
            self.needs_initial_dump = false;
        }
        let changed = self.execute(settings, action).await;
        self.advance_pending(settings).await;
        Ok(changed)
    }

    fn on_reconnected(&mut self, reconnected: bool) {
        if reconnected {
            self.subscribed = false;
            self.needs_alive = true;
            self.needs_initial_dump = false;
            self.pending.clear();
        }
    }

    async fn ensure_subscription(&mut self) -> Result<(), ClientError<C, T>> {
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
            .map_err(|_| Error::Mqtt(minimq::ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/settings/#")
            .map_err(|_| Error::Mqtt(minimq::ProtocolError::BufferSize.into()))?;
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let topics = [TopicFilter::new(&topic).options(opts)];

        let mut runner = Runner::new(
            &mut self.mqtt,
            self.connector,
            &mut self.timer,
            &mut self.connection,
        );
        runner.subscribe(&topics, &[]).await?;

        info!("Subscribed");
        self.subscribed = true;
        self.needs_initial_dump = true;
        Ok(())
    }

    async fn publish_alive(&mut self) -> Result<(), ClientError<C, T>> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(minimq::ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/alive")
            .map_err(|_| Error::Mqtt(minimq::ProtocolError::BufferSize.into()))?;
        let publication = Publication::new(&topic, self.alive.as_bytes())
            .qos(QoS::AtLeastOnce)
            .retain();
        let mut runner = Runner::new(
            &mut self.mqtt,
            self.connector,
            &mut self.timer,
            &mut self.connection,
        );
        runner.publish(publication).await.map_err(|err| match err {
            RunnerPubError::Publish(minimq::PubError::Error(err)) => Error::Mqtt(err),
            RunnerPubError::Publish(minimq::PubError::Serialization(_)) => Error::State,
            RunnerPubError::Runner(err) => err.into(),
        })?;
        Ok(())
    }

    fn plan_request(
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

        let request = match Request::parse(message) {
            Ok(request) => request,
            Err(err) => {
                return Action::ReplyText {
                    state: State::Unchanged,
                    request: Request {
                        topic: String::try_from(message.topic).unwrap_or_default(),
                        response_topic: None,
                        correlation_data: None,
                    },
                    code: ResponseCode::Error,
                    text: format_message(err),
                };
            }
        };

        if pending_active {
            return Action::ReplyText {
                state: State::Unchanged,
                request,
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
                    request,
                    code: ResponseCode::Error,
                    text: format_message(err),
                };
            }
        };

        let depth = lookup.depth;
        if message.payload.is_empty() {
            if lookup.leaf {
                Action::ReplyLeaf {
                    request,
                    state,
                    depth,
                    leaf: true,
                }
            } else if request.response_topic.is_none() {
                Action::ReplyText {
                    state: State::Unchanged,
                    request,
                    code: ResponseCode::Error,
                    text: format_message("Internal node listing requires response topic"),
                }
            } else {
                Action::StartList {
                    request,
                    state,
                    depth,
                }
            }
        } else if !lookup.leaf {
            Action::ReplyText {
                state: State::Unchanged,
                request,
                code: ResponseCode::Error,
                text: format_message("Path does not resolve to a leaf"),
            }
        } else {
            match resolve::<_, _, Y>(Settings::SCHEMA, path, |keys, _| {
                json_core::set_by_keys(settings, keys, message.payload)
            }) {
                Ok(_) => Action::ReplyText {
                    state: State::Changed,
                    request,
                    code: ResponseCode::Ok,
                    text: format_message("OK"),
                },
                Err(err) => Action::ReplyText {
                    state: State::Unchanged,
                    request,
                    code: ResponseCode::Error,
                    text: format_message(err),
                },
            }
        }
    }

    async fn execute(&mut self, settings: &Settings, action: Action<Y>) -> State {
        match action {
            Action::None(state) => state,
            Action::ReplyText {
                state,
                request,
                code,
                text,
            } => {
                self.reply_text(&request, code, text.as_str()).await;
                state
            }
            Action::ReplyLeaf {
                request,
                state,
                depth,
                leaf,
            } => {
                self.reply_leaf(settings, &request, state, depth, leaf)
                    .await;
                State::Unchanged
            }
            Action::StartList {
                request,
                state,
                depth,
            } => {
                match Pending::list(Settings::SCHEMA, &state[..depth], request.clone()) {
                    Ok(pending) => self.pending = pending,
                    Err(err) => self.reply_text(&request, ResponseCode::Error, err).await,
                }
                State::Unchanged
            }
        }
    }

    async fn reply_text(&mut self, request: &Request, code: ResponseCode, text: &str) {
        let props = [code.into()];
        let publication = request
            .reply(text.as_bytes())
            .properties(&props)
            .qos(QoS::AtLeastOnce);
        let mut runner = Runner::new(
            &mut self.mqtt,
            self.connector,
            &mut self.timer,
            &mut self.connection,
        );
        let result = runner.publish(publication).await;
        if result.is_err() {
            info!("Response failure");
        }
    }

    async fn reply_leaf(
        &mut self,
        settings: &Settings,
        request: &Request,
        state: [usize; Y],
        depth: usize,
        leaf: bool,
    ) {
        let props = [ResponseCode::Ok.into()];
        let full = &state[..depth];
        let publication = request
            .reply(|buf: &mut [u8]| {
                let mut keys = full;
                json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| DepthError {
                    inner,
                    depth: full.len() - keys.len(),
                    leaf: Some(leaf),
                })
            })
            .properties(&props)
            .qos(QoS::AtLeastOnce);

        let mut runner = Runner::new(
            &mut self.mqtt,
            self.connector,
            &mut self.timer,
            &mut self.connection,
        );
        let result = runner.publish(publication).await;

        match result {
            Ok(()) => {}
            Err(RunnerPubError::Publish(minimq::PubError::Serialization(err))) => {
                self.reply_text(request, ResponseCode::Error, format_message(err).as_str())
                    .await;
            }
            Err(_) => info!("Leaf response failure"),
        }
    }

    async fn advance_pending(&mut self, settings: &Settings) {
        while self.mqtt.can_publish(QoS::AtLeastOnce) {
            match self.pending.kind {
                PendingKind::Idle => break,
                PendingKind::List => {
                    let (request, code, payload, done) = {
                        let iter = &mut self.pending.iter;
                        let request = self.pending.request.clone().unwrap();
                        let (code, payload, done) = if let Some(path) = iter.next() {
                            let path = match path {
                                Ok(path) => path.into_inner(),
                                Err(err) => {
                                    error!("Path iter error: {err}");
                                    continue;
                                }
                            };
                            (ResponseCode::Continue, path, false)
                        } else {
                            (ResponseCode::Ok, String::new(), true)
                        };
                        (request, code, payload, done)
                    };

                    let props = [code.into()];
                    let publication = request
                        .reply(payload.as_bytes())
                        .properties(&props)
                        .qos(QoS::AtLeastOnce);
                    let mut runner = Runner::new(
                        &mut self.mqtt,
                        self.connector,
                        &mut self.timer,
                        &mut self.connection,
                    );
                    if runner.publish(publication).await.is_err() {
                        info!("Multipart list publish failure");
                        self.pending.clear();
                        break;
                    }
                    if done {
                        self.pending.clear();
                        break;
                    }
                }
                PendingKind::Dump => {
                    let Some((topic, state, depth, leaf)) = self.next_dump_step() else {
                        self.pending.clear();
                        break;
                    };

                    let props = [ResponseCode::Ok.into()];
                    let full = &state[..depth];
                    let publication = Publication::new(&topic, |buf: &mut [u8]| {
                        let mut keys = full;
                        json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| {
                            DepthError {
                                inner,
                                depth: full.len() - keys.len(),
                                leaf: Some(leaf),
                            }
                        })
                    })
                    .properties(&props)
                    .qos(QoS::AtLeastOnce);
                    let mut runner = Runner::new(
                        &mut self.mqtt,
                        self.connector,
                        &mut self.timer,
                        &mut self.connection,
                    );
                    match runner.publish(publication).await {
                        Ok(()) => {}
                        Err(RunnerPubError::Publish(minimq::PubError::Serialization(
                            DepthError {
                                inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                                ..
                            },
                        ))) => {}
                        Err(RunnerPubError::Publish(minimq::PubError::Serialization(err))) => {
                            info!("Multipart dump serialization failure: {err}");
                            self.pending.clear();
                            break;
                        }
                        Err(_) => {
                            info!("Multipart dump publish failure");
                            self.pending.clear();
                            break;
                        }
                    }
                }
            }
        }
    }

    fn next_dump_step(&mut self) -> Option<(String<MAX_TOPIC_LENGTH>, [usize; Y], usize, bool)> {
        loop {
            let path = match self.pending.iter.next()? {
                Ok(path) => path.into_inner(),
                Err(err) => {
                    error!("Path iter error: {err}");
                    continue;
                }
            };
            let full = self.pending.iter.state()?;
            let lookup = Settings::SCHEMA.get(full).unwrap();
            let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().ok()?;
            topic.push_str("/settings").ok()?;
            topic.push_str(&path).ok()?;
            let mut state = [0; Y];
            state[..full.len()].copy_from_slice(full);
            return Some((topic, state, full.len(), lookup.leaf));
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use miniconf::Tree;
    use minimq::{
        Broker, BufferLayout, Property, QoS, Retain,
        embedded_io_async::{ErrorKind, ErrorType, Read, Write},
        transport::Connector,
        types::{Properties, Utf8String},
    };

    #[derive(Tree)]
    struct Tiny {
        value: u8,
    }

    #[derive(Tree, Default)]
    struct Nested {
        leaf: u8,
    }

    #[derive(Tree, Default)]
    struct TreeSettings {
        value: u8,
        nested: Nested,
    }

    #[derive(Default)]
    struct TestTimer;

    struct DummyConnection;

    impl ErrorType for DummyConnection {
        type Error = ErrorKind;
    }

    impl Read for DummyConnection {
        async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
            Ok(0)
        }
    }

    impl Write for DummyConnection {
        async fn write(&mut self, _buf: &[u8]) -> Result<usize, Self::Error> {
            Ok(0)
        }

        async fn flush(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    struct DummyConnector;

    impl Connector for DummyConnector {
        type ConnectError = ErrorKind;
        type IoError = ErrorKind;
        type Connection<'a> = DummyConnection;

        async fn connect<'a, const N: usize>(
            &'a self,
            _broker: &Broker<N>,
        ) -> Result<Self::Connection<'a>, Self::ConnectError> {
            Ok(DummyConnection)
        }
    }

    impl Timer for TestTimer {
        type Error = ();

        fn now(&mut self) -> Result<u64, Self::Error> {
            Ok(0)
        }

        async fn sleep_until(&mut self, _deadline_ms: u64) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    #[test]
    fn constructor_rejects_long_prefix() {
        let mut buffer = [0u8; 1024];
        let broker = Broker::from("127.0.0.1".parse::<core::net::IpAddr>().unwrap());
        const MAX_DEPTH: usize = Tiny::SCHEMA.shape().max_depth;
        let prefix = "x".repeat(MAX_TOPIC_LENGTH);

        let client = MqttClient::<Tiny, _, _, MAX_DEPTH>::new(
            &prefix,
            &DummyConnector,
            TestTimer,
            minimq::ConfigBuilder::from_buffer_layout(
                broker,
                &mut buffer,
                BufferLayout {
                    rx: 256,
                    tx: 256,
                    inflight: 512,
                },
            )
            .unwrap(),
        );

        assert!(matches!(client, Err(ProtocolError::BufferSize)));
    }

    #[test]
    fn plan_leaf_get() {
        let mut settings = TreeSettings::default();
        let message = InboundPublish {
            topic: "test/id/settings/value",
            payload: b"",
            properties: Properties::Slice(&[]),
            retain: Retain::NotRetained,
            qos: QoS::AtMostOnce,
        };

        match MqttClient::<TreeSettings, DummyConnector, TestTimer, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ) {
            Action::ReplyLeaf { depth, leaf, .. } => {
                assert_eq!(depth, 1);
                assert!(leaf);
            }
            other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
        }
    }

    #[test]
    fn plan_internal_get_requires_response_topic() {
        let mut settings = TreeSettings::default();
        let message = InboundPublish {
            topic: "test/id/settings/nested",
            payload: b"",
            properties: Properties::Slice(&[]),
            retain: Retain::NotRetained,
            qos: QoS::AtMostOnce,
        };

        match MqttClient::<TreeSettings, DummyConnector, TestTimer, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ) {
            Action::StartList { .. } => panic!("internal GET without response topic must not list"),
            Action::ReplyText { code, text, .. } => {
                assert_eq!(code, ResponseCode::Error);
                assert!(text.contains("response topic"));
            }
            other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
        }
    }

    #[test]
    fn plan_set_marks_changed() {
        let mut settings = TreeSettings::default();
        let props = [Property::ResponseTopic(Utf8String("test/id/response"))];
        let message = InboundPublish {
            topic: "test/id/settings/value",
            payload: b"42",
            properties: Properties::Slice(&props),
            retain: Retain::NotRetained,
            qos: QoS::AtMostOnce,
        };

        match MqttClient::<TreeSettings, DummyConnector, TestTimer, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ) {
            Action::ReplyText { state, code, .. } => {
                assert_eq!(state, State::Changed);
                assert_eq!(code, ResponseCode::Ok);
                assert_eq!(settings.value, 42);
            }
            other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
        }
    }
}
