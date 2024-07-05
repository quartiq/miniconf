#![cfg_attr(not(any(test, doctest)), no_std)]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]
//! The Minimq MQTT client for `miniconf``.

use heapless::{String, Vec};
use log::{error, info, warn};
use miniconf::{IntoKeys, JsonCoreSlash, NodeIter, Path, Traversal, TreeKey};
pub use minimq;
use minimq::{
    embedded_nal::TcpClientStack,
    embedded_time,
    types::{SubscriptionOptions, TopicFilter},
    ConfigBuilder, DeferredPublication, ProtocolError, Publication, QoS,
};

use embedded_io::Write;

// The maximum topic length of any settings path.
const MAX_TOPIC_LENGTH: usize = 128;

// The maximum amount of correlation data that will be cached for listing. This is set to function
// with the miniconf-mqtt python client (i.e. 32 bytes can encode a UUID).
const MAX_CD_LENGTH: usize = 32;

// The delay after not receiving messages after initial connection that settings will be
// republished.
const REPUBLISH_TIMEOUT_SECONDS: u32 = 2;

const SEPARATOR: char = '/';

type Iter<M, const Y: usize> = NodeIter<M, Y, Path<String<MAX_TOPIC_LENGTH>, SEPARATOR>>;

pub enum Error<E> {
    Miniconf(miniconf::Error<()>),
    State(sm::Error),
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
            Init + Init = Multipart,

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

enum Command<'a> {
    List { path: &'a str },
    Get { path: &'a str },
    Set { path: &'a str, value: &'a [u8] },
}

impl<'a> Command<'a> {
    fn try_from_message(topic: &'a str, value: &'a [u8]) -> Result<Self, ()> {
        if topic == "/list" {
            let path = core::str::from_utf8(value).or(Err(()))?;
            Ok(Command::List { path })
        } else {
            let path = topic.strip_prefix("/settings").ok_or(())?;
            if value.is_empty() {
                Ok(Command::Get { path })
            } else {
                Ok(Command::Set { path, value })
            }
        }
    }
}

/// Cache correlation data and topic for multi-part responses.
struct Multipart<M: TreeKey<Y>, const Y: usize> {
    iter: Iter<M, Y>,
    topic: Option<String<MAX_TOPIC_LENGTH>>,
    correlation_data: Option<Vec<u8, MAX_CD_LENGTH>>,
}

impl<M: TreeKey<Y>, const Y: usize> Default for Multipart<M, Y> {
    fn default() -> Self {
        Self {
            iter: M::nodes(),
            topic: None,
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
        let topic = value
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
            topic,
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
/// use miniconf_mqtt::MqttClient;
///
/// #[derive(Tree, Clone, Default)]
/// struct Settings {
///     foo: bool,
/// }
///
/// let mut buffer = [0u8; 1024];
/// let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
/// let mut client: MqttClient<'_, _, _, _, minimq::broker::IpBroker, 1> = MqttClient::new(
///     std_embedded_nal::Stack::default(),
///     "quartiq/application/12345", // prefix
///     std_embedded_time::StandardClock::default(),
///     minimq::ConfigBuilder::new(localhost.into(), &mut buffer),
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
                    self.state.process_event(sm::Events::Subscribe).unwrap();
                }
            }
            sm::States::Wait => {
                self.state.process_event(sm::Events::Tick).unwrap();
            }
            sm::States::Init => {
                self.pending = Multipart::default();
                self.state.process_event(sm::Events::Init).unwrap();
            }
            sm::States::Multipart => self.multipart(settings),
            sm::States::Single => { // handled in poll()
            }
        }
        // All states must handle MQTT traffic.
        self.poll(settings)
    }

    /// Force republication of the current settings.
    ///
    /// # Note
    /// This is intended to be used if modification of a setting had side effects that affected
    /// another setting.
    pub fn republish(&mut self, path: &str) -> Result<(), Error<Stack::Error>> {
        self.pending = Multipart::default().root(&Path::<_, SEPARATOR>::from(path))?;
        self.state.process_event(sm::Events::Multipart)?;
        Ok(())
    }

    fn alive(&mut self) -> bool {
        // Publish a connection status message.
        let mut alive = self.prefix.clone();
        alive.push_str("/alive").unwrap();
        let msg = Publication::new(b"1")
            .topic(&alive)
            .retain()
            .finish()
            .unwrap();

        self.mqtt.client().publish(msg).is_ok()
    }

    fn subscribe(&mut self) -> bool {
        info!("MQTT connected, subscribing to settings");

        // Note(unwrap): We construct a string with two more characters than the prefix
        // structure, so we are guaranteed to have space for storage.
        let mut settings = self.prefix.clone();
        settings.push_str("/settings/#").unwrap();
        let mut list = self.prefix.clone();
        list.push_str("/list").unwrap();
        let opts = SubscriptionOptions::default().ignore_local_messages();
        let topics = [
            TopicFilter::new(&settings).options(opts),
            TopicFilter::new(&list).options(opts),
        ];

        self.mqtt.client().subscribe(&topics, &[]).is_ok()
    }

    fn handle_listing(&mut self) {
        let Some(cache) = &mut self.pending else {
            return;
        };

        while self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            // Note(unwrap): Publishing should not fail because `can_publish()` was checked before
            // attempting this publish.
            let (code, path) = match self.iter.next() {
                Some(path) => (ResponseCode::Continue, path.unwrap().0.into_inner()),
                None => (ResponseCode::Ok, String::new()),
            };

            let props = [code.into()];
            let mut outgoing = Publication::new(path.as_bytes())
                .topic(&cache.topic)
                .properties(&props)
                .qos(QoS::AtLeastOnce);

            if let Some(cd) = &cache.correlation_data {
                outgoing = outgoing.correlate(cd);
            }

            let publication = match outgoing.finish() {
                Ok(response) => response,
                Err(e) => {
                    // Something went wrong. Abort the listing.
                    error!("Listing failed to build response: {e:?}");
                    self.pending.take();
                    return;
                }
            };

            // Note(unwrap) We already checked that we can publish earlier.
            self.mqtt.client().publish(publication).unwrap();

            // If we're done with listing, bail out of the loop.
            if code != ResponseCode::Continue {
                self.pending.take();
                break;
            }
        }
    }

    fn multipart(&mut self, settings: &Settings) {
        if self.pending.is_some() {
            return;
        }

        while self.mqtt.client().can_publish(QoS::AtMostOnce) {
            let Some(path) = self.iter.next() else {
                // If we got here, we completed iterating over the topics and published them all.
                self.state.process_event(sm::Events::Complete).unwrap();
                break;
            };

            let (path, _node) = path.unwrap();

            let mut prefixed_topic = self.prefix.clone();
            prefixed_topic
                .push_str("/settings")
                .and_then(|_| prefixed_topic.push_str(&path))
                .unwrap();

            // If the topic is not present, we'll fail to serialize the setting into the
            // payload and will never publish. The iterator has already incremented, so this is
            // acceptable.
            let response = DeferredPublication::new(|buf| settings.get_json_by_key(&path, buf))
                .topic(&prefixed_topic)
                .finish()
                .unwrap();

            // Note(unwrap): This should not fail because `can_publish()` was checked before
            // attempting this publish.
            match self.mqtt.client().publish(response) {
                Err(minimq::PubError::Serialization(Error::Traversal(Traversal::Absent(_)))) => {}

                // If the value is too large to serialize, print an error to the topic instead
                Err(minimq::PubError::Error(minimq::Error::Minimq(
                    minimq::MinimqError::Protocol(minimq::ProtocolError::Serialization(
                        minimq::SerError::InsufficientMemory,
                    )),
                ))) => {
                    self.mqtt
                        .client()
                        .publish(
                            Publication::new(b"<error: serialization too large>")
                                .topic(&prefixed_topic)
                                .properties(&[ResponseCode::Error.into()])
                                .finish()
                                .unwrap(),
                        )
                        .unwrap();
                }
                other => other.unwrap(),
            }
        }
    }

    fn poll(&mut self, settings: &mut Settings) -> Result<bool, Error<Stack::Error>> {
        let mut updated = false;
        let poll = self.mqtt.poll(|client, topic, payload, properties| {
            let Some(topic) = topic.strip_prefix(self.prefix.as_str()) else {
                info!("Unexpected topic prefix: {topic}");
                return;
            };

            let Ok(command) = Command::try_from_message(topic, payload) else {
                info!("Unknown miniconf command: {topic}");
                return;
            };

            match command {
                Command::List { path } => {
                    if !properties
                        .into_iter()
                        .any(|prop| matches!(prop, Ok(minimq::Property::ResponseTopic(_))))
                    {
                        info!("Discarding `List` without `ResponseTopic`");
                        return;
                    }

                    let response = if self.pending.is_some() {
                        "`List` already in progress"
                    } else {
                        match Multipart::try_from(properties) {
                            Err(msg) => msg,
                            Ok(cache) => {
                                self.pending.replace(cache);
                                self.iter =
                                    Settings::nodes().root(&Path::<_, SEPARATOR>::from(path)).unwrap();

                                // There is no positive response sent during list commands,
                                // instead, the response is sent as a property of the listed
                                // elements. As such, we are now finished processing a list
                                // command.
                                return;
                            }
                        }
                    };

                    let props = [ResponseCode::Error.into()];
                    if let Ok(response) = minimq::Publication::new(response.as_bytes())
                        .reply(properties)
                        .properties(&props)
                        .qos(QoS::AtLeastOnce)
                        .finish()
                    {
                        client.publish(response).ok();
                    }
                }

                Command::Get { path } => {
                    let props = [ResponseCode::Ok.into()];
                    let Ok(message) = DeferredPublication::new(|buf| settings.get_json(path, buf))
                        .properties(&props)
                        .reply(properties)
                        // Override the response topic with the path.
                        .qos(QoS::AtLeastOnce)
                        .finish()
                    else {
                        // If we can't create the publication, it's because there's no way to reply
                        // to the message. Since we don't know where to send things, abort now and
                        // complete handling of the `Get` request.
                        return;
                    };

                    match client.publish(message) {
                        Ok(()) => {}
                        Err(err) => {}
                        Err(minimq::PubError::Serialization(miniconf::Error::Traversal(
                            Traversal::TooShort(depth),
                        ))) => {
                            // Internal node
                            // TODO: iter update
                        }
                        Err(minimq::PubError::Serialization(err)) => {
                            if let Ok(message) = DeferredPublication::new(|mut buf| {
                                let start = buf.len();
                                write!(buf, "{}", err).and_then(|_| Ok(start - buf.len()))
                            })
                            .properties(&[ResponseCode::Error.into()])
                            .reply(properties)
                            .qos(QoS::AtLeastOnce)
                            .finish()
                            {
                                // Try to send the error as a best-effort. If we don't have enough
                                // buffer space to encode the error, there's nothing more we can do.
                                client.publish(message).ok();
                            };
                        }
                    }
                }

                Command::Set { path, value } => match settings.set_json(path, value) {
                    Ok(_depth) => {
                        updated = true;
                        if let Ok(response) = Publication::new("OK".as_bytes())
                            .properties(&[ResponseCode::Ok.into()])
                            .reply(properties)
                            .qos(QoS::AtLeastOnce)
                            .finish()
                        {
                            client.publish(response).ok();
                        }
                    }
                    Err(err) => {
                        if let Ok(response) = DeferredPublication::new(|mut buf| {
                            let start = buf.len();
                            write!(buf, "{}", err).and_then(|_| Ok(start - buf.len()))
                        })
                        .properties(&[ResponseCode::Error.into()])
                        .reply(properties)
                        .qos(QoS::AtLeastOnce)
                        .finish()
                        {
                            client.publish(response).ok();
                        }
                    }
                },
            }
        });
        match poll {
            Ok(_) => Ok(updated),
            Err(minimq::Error::SessionReset) => {
                warn!("Session reset");
                self.state.process_event(sm::Events::Reset).unwrap();
                Ok(false)
            }
            Err(other) => Err(other.into()),
        }
    }
}
