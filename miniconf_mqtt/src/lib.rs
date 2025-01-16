#![no_std]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]
//! The Minimq MQTT client for `miniconf``.

use core::fmt::Display;

use heapless::{String, Vec};
use log::{error, info, warn};
use miniconf::{
    json, IntoKeys, Metadata, NodeIter, Path, Traversal, TreeDeserializeOwned, TreeKey,
    TreeSerialize,
};
pub use minimq;
use minimq::{
    embedded_nal::TcpClientStack,
    embedded_time,
    types::{Properties, SubscriptionOptions, TopicFilter},
    ConfigBuilder, DeferredPublication, ProtocolError, Publication, QoS,
};
use strum::IntoStaticStr;

use embedded_io::Write;

// The maximum topic length of any topic (prefix + "/settings" + miniconf path).
const MAX_TOPIC_LENGTH: usize = 128;

// The maximum amount of correlation data that will be cached for listing. This is set to function
// with the miniconf-mqtt python client (i.e. 32 bytes can encode a UUID).
const MAX_CD_LENGTH: usize = 32;

// The delay after not receiving messages after initial connection that settings will be
// dumped.
const DUMP_TIMEOUT_SECONDS: u32 = 2;

const SEPARATOR: char = '/';

/// Miniconf MQTT joint error type
#[derive(Debug, PartialEq)]
pub enum Error<E> {
    /// Miniconf
    Miniconf(miniconf::Traversal),
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

impl<E> From<miniconf::Traversal> for Error<E> {
    fn from(value: miniconf::Traversal) -> Self {
        Self::Miniconf(value)
    }
}

impl<E> From<minimq::Error<E>> for Error<E> {
    fn from(value: minimq::Error<E>) -> Self {
        Self::Minimq(value)
    }
}

mod sm {
    use super::DUMP_TIMEOUT_SECONDS;
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
                .replace(self.clock.try_now().unwrap() + DUMP_TIMEOUT_SECONDS.seconds());
            Ok(())
        }
    }
}

/// Cache correlation data and topic for multi-part responses.
struct Multipart<M, const Y: usize> {
    iter: NodeIter<M, Path<String<MAX_TOPIC_LENGTH>, SEPARATOR>, Y>,
    response_topic: Option<String<MAX_TOPIC_LENGTH>>,
    correlation_data: Option<Vec<u8, MAX_CD_LENGTH>>,
}

impl<M: TreeKey, const Y: usize> Default for Multipart<M, Y> {
    fn default() -> Self {
        Self {
            iter: M::nodes(),
            response_topic: None,
            correlation_data: None,
        }
    }
}

impl<M: TreeKey, const Y: usize> Multipart<M, Y> {
    fn root<K: IntoKeys>(mut self, keys: K) -> Result<Self, miniconf::Traversal> {
        self.iter = self.iter.root(keys)?;
        Ok(self)
    }
}

impl<M: TreeKey, const Y: usize> TryFrom<&minimq::types::Properties<'_>> for Multipart<M, Y> {
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

/// MQTT settings interface.
///
/// # Design
/// The MQTT client places the [TreeKey] paths `<path>` at the MQTT `<prefix>/settings/<path>` topic,
/// where `<prefix>` is provided in the client constructor.
///
/// By default it publishes its alive-ness as a `1` retained to `<prefix>/alive` and and clears it
/// when disconnected.
///
/// # Limitations
/// The client supports paths up to `MAX_TOPIC_LENGTH = 128` byte length.
/// Re-publication timeout is fixed to `DUMP_TIMEOUT_SECONDS = 2` seconds.
///
/// # Example
/// ```
/// use miniconf::{Leaf, Tree};
///
/// #[derive(Tree, Clone, Default)]
/// struct Settings {
///     foo: Leaf<bool>,
/// }
///
/// let mut buffer = [0u8; 1024];
/// let localhost: core::net::IpAddr = "127.0.0.1".parse().unwrap();
/// let mut client = miniconf_mqtt::MqttClient::<_, _, _, _, 1>::new(
///     std_embedded_nal::Stack::default(),
///     "quartiq/application/12345", // prefix
///     std_embedded_time::StandardClock::default(),
///     minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer),
/// )
/// .unwrap();
/// let mut settings = Settings::default();
/// client.update(&mut settings).unwrap();
/// ```
pub struct MqttClient<'a, Settings, Stack, Clock, Broker, const Y: usize>
where
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
    Broker: minimq::Broker,
{
    mqtt: minimq::Minimq<'a, Stack, Clock, Broker>,
    state: sm::StateMachine<sm::Context<Clock>>,
    prefix: &'a str,
    alive: &'a str,
    pending: Multipart<Settings, Y>,
}

impl<'a, Settings, Stack, Clock, Broker, const Y: usize>
    MqttClient<'a, Settings, Stack, Clock, Broker, Y>
where
    Settings: TreeKey + TreeSerialize + TreeDeserializeOwned,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock + Clone,
    Broker: minimq::Broker,
{
    /// Construct a new MQTT settings interface.
    ///
    /// # Args
    /// * `stack` - The network stack to use for communication.
    /// * `prefix` - The MQTT device prefix to use for this device
    /// * `clock` - The clock for managing the MQTT connection.
    /// * `config` - The configuration of the MQTT client.
    pub fn new(
        stack: Stack,
        prefix: &'a str,
        clock: Clock,
        config: ConfigBuilder<'a, Broker>,
    ) -> Result<Self, ProtocolError> {
        assert_eq!("/".len(), SEPARATOR.len_utf8());
        let meta: Metadata = Settings::traverse_all().unwrap(); // Note(unwrap): infallible
        assert!(meta.max_depth <= Y);
        assert!(prefix.len() + "/settings".len() + meta.max_length("/") <= MAX_TOPIC_LENGTH);

        // Configure a will so that we can indicate whether or not we are connected.
        let mut will: String<MAX_TOPIC_LENGTH> = prefix.try_into().unwrap();
        will.push_str("/alive").unwrap();
        // Retained empty payload amounts to clearing the retained value (see MQTT spec).
        let will = minimq::Will::new(&will, b"", &[])?
            .retained()
            .qos(QoS::AtMostOnce);
        let config = config.autodowngrade_qos().will(will)?;

        Ok(Self {
            mqtt: minimq::Minimq::new(stack, clock.clone(), config),
            state: sm::StateMachine::new(sm::Context::new(clock)),
            prefix,
            alive: "1",
            pending: Multipart::default(),
        })
    }

    /// Set the payload published on the `/alive` topic when connected to the broker.
    ///
    /// The default is to publish `1`.
    /// The message is retained by the broker.
    /// On disconnect the message is cleared retained through an MQTT will.
    pub fn set_alive(&mut self, alive: &'a str) {
        self.alive = alive;
    }

    /// Reset and restart state machine.
    ///
    /// This rests the state machine to start from the `Connect` state.
    /// This will connect (if not connected), send the alive message, subscribe,
    /// and perform the initial settings dump.
    pub fn reset(&mut self) {
        self.state.process_event(sm::Events::Reset).unwrap();
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
                if self.alive().is_ok() {
                    self.state.process_event(sm::Events::Alive).unwrap();
                }
            }
            sm::States::Subscribe => {
                if self.subscribe().is_ok() {
                    info!("Subscribed");
                    self.state.process_event(sm::Events::Subscribe).unwrap();
                }
            }
            sm::States::Wait => {
                self.state.process_event(sm::Events::Tick).ok();
            }
            sm::States::Init => {
                info!("Dumping");
                self.dump(None).ok();
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
        self.poll(settings).map(|c| c == State::Changed)
    }

    fn alive(&mut self) -> Result<(), minimq::PubError<Stack::Error, ()>> {
        // Publish a connection status message.
        let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        topic.push_str("/alive").unwrap();
        let msg = Publication::new(&topic, self.alive.as_bytes())
            .qos(QoS::AtLeastOnce)
            .retain();
        self.mqtt.client().publish(msg)
    }

    fn subscribe(&mut self) -> Result<(), minimq::Error<Stack::Error>> {
        let mut settings: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        settings.push_str("/settings/#").unwrap();
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let topics = [TopicFilter::new(&settings).options(opts)];
        self.mqtt.client().subscribe(&topics, &[])
    }

    /// Dump the current settings.
    ///
    /// # Note
    /// This is intended to be used if modification of a setting had side effects that affected
    /// another setting.
    pub fn dump(&mut self, path: Option<&str>) -> Result<(), Error<Stack::Error>> {
        let mut m = Multipart::default();
        if let Some(path) = path {
            m = m.root(Path::<_, SEPARATOR>::from(path))?;
        }
        self.state.process_event(sm::Events::Multipart)?;
        self.pending = m;
        Ok(())
    }

    fn iter_list(&mut self) {
        while self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            let (code, path) = if let Some(path) = self.pending.iter.next() {
                let (path, node) = path.unwrap(); // Note(unwrap) checked capacity
                debug_assert!(node.is_leaf()); // Note(assert): Iterator depth unlimited
                (ResponseCode::Continue, path.into_inner())
            } else {
                (ResponseCode::Ok, String::new())
            };

            let props = [code.into()];
            let mut response = Publication::new(
                self.pending.response_topic.as_ref().unwrap(),
                path.as_bytes(),
            )
            .properties(&props)
            .qos(QoS::AtLeastOnce);

            if let Some(cd) = &self.pending.correlation_data {
                response = response.correlate(cd);
            }

            self.mqtt
                .client()
                .publish(response) // Note(unwrap): has topic
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
            debug_assert!(node.is_leaf()); // Note(assert): Iterator depth unlimited

            let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
            topic
                .push_str("/settings")
                .and_then(|_| topic.push_str(&path))
                .unwrap();

            let props = [ResponseCode::Ok.into()];
            let mut response =
                DeferredPublication::new(&topic, |buf| json::get_by_key(settings, &path, buf))
                    .properties(&props)
                    .qos(QoS::AtLeastOnce);

            if let Some(cd) = &self.pending.correlation_data {
                response = response.correlate(cd);
            }

            // Note(unwrap): has topic
            match self.mqtt.client().publish(response) {
                Err(minimq::PubError::Serialization(miniconf::Error::Traversal(
                    Traversal::Absent(_),
                ))) => {}

                Err(minimq::PubError::Error(minimq::Error::Minimq(
                    minimq::MinimqError::Protocol(minimq::ProtocolError::Serialization(
                        minimq::SerError::InsufficientMemory,
                    )),
                ))) => {
                    let props = [ResponseCode::Error.into()];
                    let mut response =
                        Publication::new(&topic, "Serialized value too large".as_bytes())
                            .properties(&props)
                            .qos(QoS::AtLeastOnce);

                    if let Some(cd) = &self.pending.correlation_data {
                        response = response.correlate(cd);
                    }

                    self.mqtt
                        .client()
                        .publish(response) // Note(unwrap): has topic
                        .unwrap(); // Note(unwrap): checked can_publish, error message is short
                }
                other => other.unwrap(),
            }
        }
    }

    fn respond<'b, T: Display>(
        response: T,
        code: ResponseCode,
        request: &Properties<'b>,
        client: &mut minimq::mqtt_client::MqttClient<'a, Stack, Clock, Broker>,
    ) -> Result<
        (),
        minimq::PubError<Stack::Error, embedded_io::WriteFmtError<embedded_io::SliceWriteError>>,
    > {
        client
            .publish(
                DeferredPublication::respond(request, |mut buf| {
                    let start = buf.len();
                    write!(buf, "{}", response).and_then(|_| Ok(start - buf.len()))
                })
                .unwrap()
                .properties(&[code.into()])
                .qos(QoS::AtLeastOnce),
            )
            .inspect_err(|err| {
                info!("Response failure: {err:?}");
            })
    }

    fn poll(&mut self, settings: &mut Settings) -> Result<State, Error<Stack::Error>> {
        let Self {
            mqtt,
            state,
            prefix,
            pending,
            ..
        } = self;
        mqtt.poll(|client, topic, payload, properties| {
            let Some(path) = topic
                .strip_prefix(*prefix)
                .and_then(|p| p.strip_prefix("/settings"))
                .map(Path::<_, SEPARATOR>::from)
            else {
                info!("Unexpected topic: {topic}");
                return State::Unchanged;
            };

            if payload.is_empty() {
                // Get, Dump, or List
                // Try a Get assuming a leaf node
                if let Err(err) = client.publish(
                    DeferredPublication::respond(properties, |buf| {
                        json::get_by_key(settings, path, buf)
                    })
                    .unwrap()
                    .properties(&[ResponseCode::Ok.into()])
                    .qos(QoS::AtLeastOnce),
                ) {
                    match err {
                        minimq::PubError::Serialization(miniconf::Error::Traversal(
                            Traversal::TooShort(_depth),
                        )) => {
                            // Internal node: Dump or List
                            (state.state() != &sm::States::Single)
                                .then_some("Pending multipart response")
                                .or_else(|| {
                                    Multipart::try_from(properties)
                                        .map(|m| {
                                            *pending = m.root(path).unwrap(); // Note(unwrap) checked that it's TooShort but valid leaf
                                            state.process_event(sm::Events::Multipart).unwrap();
                                            // Responses come through iter_list/iter_dump
                                        })
                                        .err()
                                })
                                .map(|msg| {
                                    Self::respond(msg, ResponseCode::Error, properties, client).ok()
                                });
                        }
                        minimq::PubError::Serialization(err) => {
                            Self::respond(err, ResponseCode::Error, properties, client).ok();
                        }
                        minimq::PubError::Error(minimq::Error::NotReady) => {
                            warn!("Not ready during Get. Discarding.");
                        }
                        minimq::PubError::Error(err) => {
                            error!("Get failure: {err:?}");
                        }
                    }
                }
                State::Unchanged
            } else {
                // Set
                match json::set_by_key(settings, path, payload) {
                    Err(err) => {
                        Self::respond(err, ResponseCode::Error, properties, client).ok();
                        State::Unchanged
                    }
                    Ok(_depth) => {
                        Self::respond("OK", ResponseCode::Ok, properties, client).ok();
                        State::Changed
                    }
                }
            }
        })
        .map(Option::unwrap_or_default)
        .or_else(|err| match err {
            minimq::Error::SessionReset => {
                warn!("Session reset");
                self.state.process_event(sm::Events::Reset).unwrap();
                Ok(State::Unchanged)
            }
            other => Err(other.into()),
        })
    }
}

#[derive(Default, Copy, Clone, PartialEq, PartialOrd)]
enum State {
    #[default]
    Unchanged,
    Changed,
}

impl From<bool> for State {
    fn from(value: bool) -> Self {
        if value {
            Self::Changed
        } else {
            Self::Unchanged
        }
    }
}
