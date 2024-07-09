#![cfg_attr(not(any(test, doctest)), no_std)]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]
//! The Minimq MQTT client for `miniconf``.

use core::fmt::Display;

use heapless::{String, Vec};
use log::{debug, error, info, warn};
use miniconf::{IntoKeys, JsonCoreSlash, NodeIter, Path, Traversal, TreeKey};
pub use minimq;
use minimq::{
    embedded_nal::TcpClientStack,
    embedded_time,
    types::{Properties, SubscriptionOptions, TopicFilter},
    ConfigBuilder, DeferredPublication, ProtocolError, Publication, QoS,
};

use embedded_io::Write;

// The maximum topic length of any topic (prefix + "/settings" + miniconf path).
const MAX_TOPIC_LENGTH: usize = 128;

// The maximum amount of correlation data that will be cached for listing. This is set to function
// with the miniconf-mqtt python client (i.e. 32 bytes can encode a UUID).
const MAX_CD_LENGTH: usize = 32;

// The delay after not receiving messages after initial connection that settings will be
// republished.
const REPUBLISH_TIMEOUT_SECONDS: u32 = 2;

const SEPARATOR: char = '/';

type Iter<M, const Y: usize> = NodeIter<M, Y, Path<String<MAX_TOPIC_LENGTH>, SEPARATOR>>;

/// Miniconf MQTT joint error type
#[derive(Debug, PartialEq)]
pub enum Error<E> {
    /// Miniconf
    Miniconf(miniconf::Error<()>),
    /// State machine
    State(sm::Error),
    /// Minimq
    Minimq(minimq::Error<E>),
}

impl<E> From<sm::Error> for Error<E> {
    fn from(value: sm::Error) -> Self {
        Self::State(value)
    }
}

impl<E, F> From<miniconf::Error<F>> for Error<E> {
    fn from(value: miniconf::Error<F>) -> Self {
        Self::Miniconf(match value {
            miniconf::Error::Finalization(_) => miniconf::Error::Finalization(()),
            miniconf::Error::Inner(depth, _) => miniconf::Error::Inner(depth, ()),
            miniconf::Error::Traversal(t) => miniconf::Error::Traversal(t),
            _ => unimplemented!(),
        })
    }
}

impl<E> From<miniconf::Traversal> for Error<E> {
    fn from(value: miniconf::Traversal) -> Self {
        Self::Miniconf(value.into())
    }
}

impl<E> From<minimq::Error<E>> for Error<E> {
    fn from(value: minimq::Error<E>) -> Self {
        Self::Minimq(value)
    }
}

mod sm {
    use super::REPUBLISH_TIMEOUT_SECONDS;
    use minimq::embedded_time::{self, duration::Extensions, Instant};
    use smlang::statemachine;

    statemachine! {
        transitions: {
            *Connect + Connect = Alive,
            Alive + Alive = Subscribe,
            Subscribe + Subscribe / start_timeout = Wait,
            Wait + Tick [timed_out] = Init,
            Init + Multipart = Multipart,
            Multipart + Complete = Single,
            Single + Multipart = Multipart,
            _ + Reset = Connect,
        }
    }

    pub struct Context<C: embedded_time::Clock> {
        clock: C,
        timeout: Option<Instant<C>>,
    }

    impl<C: embedded_time::Clock> Context<C> {
        pub fn new(clock: C) -> Self {
            Self {
                clock,
                timeout: None,
            }
        }
    }

    impl<C: embedded_time::Clock> StateMachineContext for Context<C> {
        fn timed_out(&self) -> Result<bool, ()> {
            Ok(self
                .timeout
                .map(|t| self.clock.try_now().unwrap() >= t)
                .unwrap_or_default())
        }

        fn start_timeout(&mut self) -> Result<(), ()> {
            self.timeout
                .replace(self.clock.try_now().unwrap() + REPUBLISH_TIMEOUT_SECONDS.seconds());
            Ok(())
        }
    }
}

/// Cache correlation data and topic for multi-part responses.
struct Multipart<M: TreeKey<Y>, const Y: usize> {
    iter: Iter<M, Y>,
    response_topic: Option<String<MAX_TOPIC_LENGTH>>,
    correlation_data: Option<Vec<u8, MAX_CD_LENGTH>>,
}

impl<M: TreeKey<Y>, const Y: usize> Default for Multipart<M, Y> {
    fn default() -> Self {
        Self {
            iter: M::nodes(),
            response_topic: None,
            correlation_data: None,
        }
    }
}

impl<M: TreeKey<Y>, const Y: usize> Multipart<M, Y> {
    fn root<K: IntoKeys>(mut self, keys: K) -> Result<Self, miniconf::Traversal> {
        self.iter = self.iter.root(keys)?;
        Ok(self)
    }
}

impl<M: TreeKey<Y>, const Y: usize> TryFrom<&minimq::types::Properties<'_>> for Multipart<M, Y> {
    type Error = &'static str;
    fn try_from(value: &minimq::types::Properties<'_>) -> Result<Self, Self::Error> {
        let response_topic = value
            .into_iter()
            .response_topic()
            .map(TryInto::try_into)
            .transpose()
            .or(Err("Response topic too long"))?;
        let correlation_data = value
            .into_iter()
            .find_map(|prop| {
                if let Ok(minimq::Property::CorrelationData(cd)) = prop {
                    Some(Vec::try_from(cd.0))
                } else {
                    None
                }
            })
            .transpose()
            .or(Err("Correlation data too long"))?;
        Ok(Self {
            iter: M::nodes(),
            response_topic,
            correlation_data,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum ResponseCode {
    Ok,
    Continue,
    Error,
}

impl From<ResponseCode> for minimq::Property<'static> {
    fn from(value: ResponseCode) -> Self {
        let string = match value {
            ResponseCode::Ok => "Ok",
            ResponseCode::Continue => "Continue",
            ResponseCode::Error => "Error",
        };

        minimq::Property::UserProperty(
            minimq::types::Utf8String("code"),
            minimq::types::Utf8String(string),
        )
    }
}

/// MQTT settings interface.
///
/// # Design
/// The MQTT client places the [TreeKey] paths `<path>` at the MQTT `<prefix>/settings/<path>` topic,
/// where `<prefix>` is provided in the client constructor.
///
/// It publishes its alive-ness as a `1` to `<prefix>/alive` and sets a will to publish `0` there when
/// it is disconnected.
///
/// # Limitations
/// The MQTT client logs failures to subscribe to the settings topic, but does not re-attempt to
/// connect to it when errors occur.
///
/// The client only supports paths up to `MAX_TOPIC_LENGTH = 128` byte length.
/// Re-publication timeout is fixed to `REPUBLISH_TIMEOUT_SECONDS = 2` seconds.
///
/// # Example
/// ```
/// use miniconf::Tree;
///
/// #[derive(Tree, Clone, Default)]
/// struct Settings {
///     foo: bool,
/// }
///
/// let mut buffer = [0u8; 1024];
/// let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
/// let mut client = miniconf_mqtt::MqttClient::new(
///     std_embedded_nal::Stack::default(),
///     "quartiq/application/12345", // prefix
///     std_embedded_time::StandardClock::default(),
///     minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer),
/// )
/// .unwrap();
/// let mut settings = Settings::default();
/// client.update(&mut settings).unwrap();
/// ```
pub struct MqttClient<'buf, Settings, Stack, Clock, Broker, const Y: usize>
where
    Settings: TreeKey<Y>,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
    Broker: minimq::Broker,
{
    mqtt: minimq::Minimq<'buf, Stack, Clock, Broker>,
    state: sm::StateMachine<sm::Context<Clock>>,
    prefix: String<MAX_TOPIC_LENGTH>,
    pending: Multipart<Settings, Y>,
}

impl<'buf, Settings, Stack, Clock, Broker, const Y: usize>
    MqttClient<'buf, Settings, Stack, Clock, Broker, Y>
where
    for<'de> Settings: JsonCoreSlash<'de, Y> + Clone,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock + Clone,
    Broker: minimq::Broker,
{
    /// Construct a new MQTT settings interface.
    ///
    /// # Args
    /// * `stack` - The network stack to use for communication.
    /// * `prefix` - The MQTT device prefix to use for this device.
    /// * `clock` - The clock for managing the MQTT connection.
    /// * `config` - The configuration of the MQTT client.
    pub fn new(
        stack: Stack,
        prefix: &str,
        clock: Clock,
        config: ConfigBuilder<'buf, Broker>,
    ) -> Result<Self, ProtocolError> {
        assert!(
            prefix.len() + "/settings".len() + Settings::metadata().max_length("/")
                <= MAX_TOPIC_LENGTH
        );

        // Configure a will so that we can indicate whether or not we are connected.
        let prefix = String::try_from(prefix).unwrap();
        let mut alive = prefix.clone();
        alive.push_str("/alive").unwrap();
        let will = minimq::Will::new(&alive, b"0", &[])?
            .retained()
            .qos(QoS::AtMostOnce);
        let config = config.autodowngrade_qos().will(will)?;

        Ok(Self {
            mqtt: minimq::Minimq::new(stack, clock.clone(), config),
            state: sm::StateMachine::new(sm::Context::new(clock)),
            prefix,
            pending: Multipart::default(),
        })
    }

    /// Update the MQTT interface and service the network.
    ///
    /// # Returns
    /// True if the settings changed. False otherwise.
    pub fn update(&mut self, settings: &mut Settings) -> Result<bool, Error<Stack::Error>> {
        if !self.mqtt.client().is_connected() {
            // Note(unwrap): It's always safe to reset.
            self.state.process_event(sm::Events::Reset).unwrap();
        }

        match self.state.state() {
            sm::States::Connect => {
                if self.mqtt.client().is_connected() {
                    info!("Connected");
                    self.state.process_event(sm::Events::Connect).unwrap();
                }
            }
            sm::States::Alive => {
                if self.alive() {
                    self.state.process_event(sm::Events::Alive).unwrap();
                }
            }
            sm::States::Subscribe => {
                if self.subscribe() {
                    info!("Subscribed");
                    self.state.process_event(sm::Events::Subscribe).unwrap();
                }
            }
            sm::States::Wait => {
                self.state.process_event(sm::Events::Tick).ok();
            }
            sm::States::Init => {
                info!("Republishing");
                self.publish(None).ok();
            }
            sm::States::Multipart => {
                if self.pending.response_topic.is_some() {
                    self.iter_list();
                } else {
                    self.iter_dump(settings);
                }
            }
            sm::States::Single => { // handled in poll()
            }
        }
        // All states must handle MQTT traffic.
        self.poll(settings).map(|c| c == Changed::Changed)
    }

    fn alive(&mut self) -> bool {
        // Publish a connection status message.
        let mut alive = self.prefix.clone();
        alive.push_str("/alive").unwrap();
        let msg = Publication::new(b"1")
            .topic(&alive)
            .qos(QoS::AtLeastOnce)
            .retain()
            .finish()
            .unwrap(); // Note(unwrap): has topic
        self.mqtt.client().publish(msg).is_ok()
    }

    fn subscribe(&mut self) -> bool {
        let mut settings = self.prefix.clone();
        settings.push_str("/settings/#").unwrap();
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let topics = [TopicFilter::new(&settings).options(opts)];
        self.mqtt.client().subscribe(&topics, &[]).is_ok()
    }

    /// Force republication of the current settings.
    ///
    /// # Note
    /// This is intended to be used if modification of a setting had side effects that affected
    /// another setting.
    pub fn publish(&mut self, path: Option<&str>) -> Result<(), Error<Stack::Error>> {
        let mut m = Multipart::default();
        if let Some(path) = path {
            m = m.root(&Path::<_, SEPARATOR>::from(path))?;
        }
        self.state.process_event(sm::Events::Multipart)?;
        self.pending = m;
        Ok(())
    }

    fn iter_list(&mut self) {
        while self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            let (code, path) = self
                .pending
                .iter
                .next()
                .map(|path| {
                    let (path, node) = path.unwrap(); // Note(unwrap) checked capacity
                    assert!(node.is_leaf()); // Note(assert): Iterator depth unlimited
                    (ResponseCode::Continue, path.into_inner())
                })
                .unwrap_or((ResponseCode::Ok, String::new()));

            let props = [code.into()];
            let mut response = Publication::new(path.as_bytes())
                .topic(self.pending.response_topic.as_ref().unwrap()) // Note(unwrap) checked in update()
                .properties(&props)
                .qos(QoS::AtLeastOnce);

            if let Some(cd) = &self.pending.correlation_data {
                response = response.correlate(cd);
            }

            self.mqtt
                .client()
                .publish(response.finish().unwrap()) // Note(unwrap): has topic
                .unwrap(); // Note(unwrap) checked can_publish()

            if code != ResponseCode::Continue {
                self.state.process_event(sm::Events::Complete).unwrap();
                break;
            }
        }
    }

    fn iter_dump(&mut self, settings: &Settings) {
        while self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            let Some(path) = self.pending.iter.next() else {
                self.state.process_event(sm::Events::Complete).unwrap();
                break;
            };

            let (path, node) = path.unwrap(); // Note(unwraped): checked capacity
            assert!(node.is_leaf()); // Note(assert): Iterator depth unlimited

            let mut topic = self.prefix.clone();
            topic
                .push_str("/settings")
                .and_then(|_| topic.push_str(&path))
                .unwrap();

            let props = [ResponseCode::Ok.into()];
            let mut response = DeferredPublication::new(|buf| settings.get_json_by_key(&path, buf))
                .topic(&topic)
                .properties(&props)
                .qos(QoS::AtLeastOnce);

            if let Some(cd) = &self.pending.correlation_data {
                response = response.correlate(cd);
            }

            // Note(unwrap): has topic
            match self.mqtt.client().publish(response.finish().unwrap()) {
                Err(minimq::PubError::Serialization(miniconf::Error::Traversal(
                    Traversal::Absent(_),
                ))) => {}

                Err(minimq::PubError::Error(minimq::Error::Minimq(
                    minimq::MinimqError::Protocol(minimq::ProtocolError::Serialization(
                        minimq::SerError::InsufficientMemory,
                    )),
                ))) => {
                    let props = [ResponseCode::Error.into()];
                    let mut response = Publication::new(b"Serialized value too large")
                        .topic(&topic)
                        .properties(&props)
                        .qos(QoS::AtLeastOnce);

                    if let Some(cd) = &self.pending.correlation_data {
                        response = response.correlate(cd);
                    }

                    self.mqtt
                        .client()
                        .publish(response.finish().unwrap()) // Note(unwrap): has topic
                        .unwrap(); // Note(unwrap): checked can_publish, error message is short
                }
                other => other.unwrap(),
            }
        }
    }

    fn respond<'a, T: Display>(
        response: T,
        code: ResponseCode,
        request: &Properties<'a>,
        client: &mut minimq::mqtt_client::MqttClient<'buf, Stack, Clock, Broker>,
    ) -> Result<
        (),
        minimq::PubError<Stack::Error, embedded_io::WriteFmtError<embedded_io::SliceWriteError>>,
    > {
        client
            .publish(
                DeferredPublication::new(|mut buf| {
                    let start = buf.len();
                    write!(buf, "{}", response).and_then(|_| Ok(start - buf.len()))
                })
                .reply(request)
                .properties(&[code.into()])
                .qos(QoS::AtLeastOnce)
                .finish()
                .map_err(minimq::Error::from)?,
            )
            .map_err(|err| {
                debug!("Response failure: {err:?}");
                err
            })
    }

    fn poll(&mut self, settings: &mut Settings) -> Result<Changed, Error<Stack::Error>> {
        let Self {
            mqtt,
            state,
            prefix,
            pending,
        } = self;
        mqtt.poll(|client, topic, payload, properties| {
            let Some(path) = topic
                .strip_prefix(prefix.as_str())
                .and_then(|p| p.strip_prefix("/settings"))
                .map(Path::<_, SEPARATOR>::from)
            else {
                info!("Unexpected topic: {topic}");
                return Changed::Unchanged;
            };

            if payload.is_empty() {
                // Get, Dump, or List
                // Try a Get assuming a leaf node
                if let Err(err) = client.publish(
                    DeferredPublication::new(|buf| settings.get_json_by_key(&path, buf))
                        .topic(topic)
                        .reply(properties)
                        .properties(&[ResponseCode::Ok.into()])
                        .qos(QoS::AtLeastOnce)
                        .finish()
                        .unwrap(), // Note(unwrap): has topic
                ) {
                    match err {
                        minimq::PubError::Serialization(miniconf::Error::Traversal(
                            Traversal::TooShort(_depth),
                        )) => {
                            // Internal node: Dump or List
                            (state.state() == &sm::States::Single)
                                .then_some(())
                                .ok_or("Pending multipart response")
                                .and_then(|()| Multipart::try_from(properties))
                                .map_or_else(
                                    |err| {
                                        Self::respond(err, ResponseCode::Error, properties, client)
                                            .ok();
                                    },
                                    |m| {
                                        *pending = m.root(&path).unwrap(); // Note(unwrap) checked that it's TooShort but valid leaf
                                        state.process_event(sm::Events::Multipart).unwrap();
                                        // Responses comes through iter_list/iter_dump
                                    },
                                );
                        }
                        minimq::PubError::Serialization(err) => {
                            Self::respond(err, ResponseCode::Error, properties, client).ok();
                        }
                        minimq::PubError::Error(err) => {
                            error!("Get failure: {err:?}");
                        }
                    }
                }
                Changed::Unchanged
            } else {
                // Set
                settings
                    .set_json_by_key(&path, payload)
                    .map_err(|err| Self::respond(err, ResponseCode::Error, properties, client).ok())
                    .map(|_depth| Self::respond("OK", ResponseCode::Ok, properties, client).ok())
                    .is_ok()
                    .into()
            }
        })
        .map(Option::unwrap_or_default)
        .or_else(|err| match err {
            minimq::Error::SessionReset => {
                warn!("Session reset");
                self.state.process_event(sm::Events::Reset).unwrap();
                Ok(Changed::Unchanged)
            }
            other => Err(other.into()),
        })
    }
}

#[derive(Default, Copy, Clone, PartialEq, PartialOrd)]
enum Changed {
    #[default]
    Unchanged,
    Changed,
}

impl From<bool> for Changed {
    fn from(value: bool) -> Self {
        if value {
            Self::Changed
        } else {
            Self::Unchanged
        }
    }
}
