#![no_std]
#![warn(missing_docs)]

//! Async MQTT interface for `miniconf`.

use core::{
    convert::Infallible,
    fmt::{Display, Write as FmtWrite},
    marker::PhantomData,
};

use heapless::String;
use log::{error, info, warn};
use miniconf::{
    DescendError, IntoKeys, Lookup, NodeIter, Path, Schema, SerdeError, TreeDeserializeOwned,
    TreeSchema, TreeSerialize, ValueError, json_core,
};
pub use minimq;
use minimq::{
    ConfigBuilder, Event, InboundPublish, OwnedResponseTarget, ProtocolError, Publication, QoS,
    Session,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};
use strum::IntoStaticStr;

const MAX_TOPIC_LENGTH: usize = 128;
const MAX_RESPONSE_LENGTH: usize = 128;
const SEPARATOR: char = '/';

/// Miniconf MQTT error.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// A multipart operation is already in progress.
    Busy,
    /// Miniconf path resolution failed.
    Miniconf(DescendError<()>),
    /// MQTT or transport operation failed.
    Mqtt(minimq::Error),
}

impl From<DescendError<()>> for Error {
    fn from(value: DescendError<()>) -> Self {
        Self::Miniconf(value)
    }
}

impl From<minimq::Error> for Error {
    fn from(value: minimq::Error) -> Self {
        Self::Mqtt(value)
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

type Request = Option<OwnedResponseTarget<MAX_TOPIC_LENGTH, 32>>;

fn parse_request(message: &InboundPublish<'_>) -> Result<Request, &'static str> {
    message
        .reply_owned()
        .map_err(|_| "Response topic or correlation data too long")
}

fn request_publication<P>(request: &Request, payload: P) -> Option<Publication<'_, P>> {
    request.as_ref().map(|target| target.publication(payload))
}

fn requires_reply(request: &Request) -> bool {
    request.is_some()
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
        if !requires_reply(&request) {
            return Err("Internal node listing requires response topic");
        }
        let iter = NodeIter::with_root(schema, root, SEPARATOR).map_err(|_| "Invalid list root")?;
        Ok(Self {
            kind: PendingKind::List,
            iter,
            request: Some(request),
        })
    }

    fn dump_root(schema: &'static Schema, root: &[usize]) -> Result<Self, DescendError<()>> {
        Ok(Self {
            kind: PendingKind::Dump,
            iter: NodeIter::with_root(schema, root, SEPARATOR)?,
            request: None,
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
    StartDump {
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

fn simple_pub_error(err: minimq::PubError<()>) -> Error {
    match err {
        minimq::PubError::Error(err) => Error::Mqtt(err),
        minimq::PubError::Serialization(()) => Error::Mqtt(ProtocolError::BufferSize.into()),
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
        let will = minimq::Will::new_owned(&will_topic, b"", &[])?
            .retained()
            .qos(QoS::AtMostOnce);
        let config = config.autodowngrade_qos().will(will)?.build();

        Ok(Self {
            session: Session::new(config, connector),
            prefix,
            alive: "1",
            subscribed: false,
            needs_alive: true,
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
        if reconnected {
            self.pending.clear();
        }
        self.subscribed = false;
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

        let request = match parse_request(message) {
            Ok(request) => request,
            Err(err) => {
                warn!("Discarding request {}: {err}", message.topic);
                return Action::None(State::Unchanged);
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

        if message.payload.is_empty() {
            if lookup.leaf {
                Action::ReplyLeaf {
                    request,
                    state,
                    depth: lookup.depth,
                    leaf: true,
                }
            } else if requires_reply(&request) {
                Action::StartList {
                    request,
                    state,
                    depth: lookup.depth,
                }
            } else {
                Action::StartDump {
                    state,
                    depth: lookup.depth,
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
            Action::StartDump { state, depth } => {
                match Pending::dump_root(Settings::SCHEMA, &state[..depth]) {
                    Ok(pending) => self.pending = pending,
                    Err(err) => info!("Dump scheduling failure: {err}"),
                }
                State::Unchanged
            }
        }
    }

    async fn reply_text(&mut self, request: &Request, code: ResponseCode, text: &str) {
        let Some(publication) = request_publication(request, text.as_bytes()) else {
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
        request: &Request,
        state: [usize; Y],
        depth: usize,
        leaf: bool,
    ) {
        let Some(publication) = request_publication(request, |buf: &mut [u8]| {
            let full = &state[..depth];
            let mut keys = full;
            json_core::get_by_keys(settings, &mut keys, buf).map_err(|inner| DepthError {
                inner,
                depth: full.len() - keys.len(),
                leaf: Some(leaf),
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
                self.reply_text(request, ResponseCode::Error, format_message(err).as_str())
                    .await;
            }
            Err(minimq::PubError::Error(err)) => info!("Leaf response failure: {err:?}"),
        }
    }

    async fn advance_pending(&mut self, settings: &Settings) {
        while self.session.can_publish(QoS::AtLeastOnce) {
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

                    let Some(publication) = request_publication(&request, payload.as_bytes()) else {
                        self.pending.clear();
                        break;
                    };
                    let props = [code.into()];
                    if let Err(err) = self
                        .session
                        .publish(publication.properties(&props).qos(QoS::AtLeastOnce))
                        .await
                    {
                        info!(
                            "Multipart list publish failure: {:?}",
                            simple_pub_error(err)
                        );
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
                    let publication = Publication::new(&topic, |buf: &mut [u8]| {
                        let full = &state[..depth];
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

                    match self.session.publish(publication).await {
                        Ok(()) => {}
                        Err(minimq::PubError::Serialization(DepthError {
                            inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                            ..
                        })) => {}
                        Err(minimq::PubError::Serialization(err)) => {
                            info!("Multipart dump serialization failure: {err}");
                            self.pending.clear();
                            break;
                        }
                        Err(minimq::PubError::Error(err)) => {
                            info!("Multipart dump publish failure: {err:?}");
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
        type Error = ErrorKind;
        type Connection<'a> = DummyConnection;

        async fn connect<'a>(
            &'a self,
            _broker: &Broker,
        ) -> Result<Self::Connection<'a>, minimq::Error> {
            Ok(DummyConnection)
        }
    }

    #[test]
    fn constructor_rejects_long_prefix() {
        let mut buffer = [0u8; 1024];
        let broker = Broker::from("127.0.0.1".parse::<core::net::IpAddr>().unwrap());
        const MAX_DEPTH: usize = Tiny::SCHEMA.shape().max_depth;
        let prefix = "x".repeat(MAX_TOPIC_LENGTH);

        let client = MqttClient::<Tiny, _, MAX_DEPTH>::new(
            &prefix,
            &DummyConnector,
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

        match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
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
    fn plan_internal_get_without_response_topic_starts_dump() {
        let mut settings = TreeSettings::default();
        let message = InboundPublish {
            topic: "test/id/settings/nested",
            payload: b"",
            properties: Properties::Slice(&[]),
            retain: Retain::NotRetained,
            qos: QoS::AtMostOnce,
        };

        match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ) {
            Action::StartDump { depth, .. } => assert_eq!(depth, 1),
            other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
        }
    }

    #[test]
    fn plan_internal_get_with_response_topic_starts_list() {
        let mut settings = TreeSettings::default();
        let props = [Property::ResponseTopic(Utf8String("test/id/response"))];
        let message = InboundPublish {
            topic: "test/id/settings/nested",
            payload: b"",
            properties: Properties::Slice(&props),
            retain: Retain::NotRetained,
            qos: QoS::AtMostOnce,
        };

        match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ) {
            Action::StartList { depth, .. } => assert_eq!(depth, 1),
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

        match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
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
