use crate::{Error, JsonCoreSlash, TreeKey};
use heapless::{String, Vec};
use minimq::{
    embedded_nal::TcpClientStack,
    embedded_time,
    types::{SubscriptionOptions, TopicFilter},
    DeferredPublication, Publication, QoS,
};

use embedded_io::Write;

// The maximum topic length of any settings path.
const MAX_TOPIC_LENGTH: usize = 128;

// The maximum amount of correlation data that will be cached for listing. This is set to function
// with the Miniconf python client (i.e. 32 bytes can encode a UUID).
const MAX_CD_LENGTH: usize = 32;

// The delay after not receiving messages after initial connection that settings will be
// republished.
const REPUBLISH_TIMEOUT_SECONDS: u32 = 2;

type Iter<M, const Y: usize> = crate::PathIter<'static, M, Y, String<MAX_TOPIC_LENGTH>>;

mod sm {
    use super::{Iter, TreeKey, REPUBLISH_TIMEOUT_SECONDS};
    use minimq::embedded_time::{self, duration::Extensions, Instant};
    use smlang::statemachine;

    statemachine! {
        transitions: {
            *Initial + Connected = ConnectedToBroker,
            ConnectedToBroker + IndicatedLife = PendingSubscribe,

            // After initial subscriptions, we start a timeout to republish all settings.
            PendingSubscribe + Subscribed / start_republish_timeout = PendingRepublish,

            // Settings republish can be completed any time after subscription.
            PendingRepublish + StartRepublish / start_republish = RepublishingSettings,
            RepublishingSettings + StartRepublish / start_republish = RepublishingSettings,
            Active + StartRepublish / start_republish = RepublishingSettings,

            // After republishing settings, we are in an idle "active" state.
            RepublishingSettings + Complete = Active,

            // All states transition back to `initial` on reset.
            _ + Reset = Initial,
        }
    }

    pub struct Context<C: embedded_time::Clock, M: TreeKey<Y>, const Y: usize> {
        clock: C,
        timeout: Option<Instant<C>>,
        pub republish_state: Iter<M, Y>,
    }

    impl<C: embedded_time::Clock, M: TreeKey<Y>, const Y: usize> Context<C, M, Y> {
        pub fn new(clock: C) -> Self {
            Self {
                clock,
                timeout: None,
                // Skip redundant check (done comprehensively in `MqttClient::new()`)
                republish_state: M::iter_paths_unchecked("/"),
            }
        }

        pub fn republish_has_timed_out(&self) -> bool {
            if let Some(timeout) = self.timeout {
                self.clock.try_now().unwrap() > timeout
            } else {
                false
            }
        }
    }

    impl<C: embedded_time::Clock, M: TreeKey<Y>, const Y: usize> StateMachineContext
        for Context<C, M, Y>
    {
        fn start_republish_timeout(&mut self) {
            self.timeout
                .replace(self.clock.try_now().unwrap() + REPUBLISH_TIMEOUT_SECONDS.seconds());
        }

        fn start_republish(&mut self) {
            // Skip redundant check (done comprehensively in `MqttClient::new()`)
            self.republish_state = M::iter_paths_unchecked("/");
        }
    }
}

enum Command<'a> {
    List,
    Get { path: &'a str },
    Set { path: &'a str, value: &'a [u8] },
}

impl<'a> Command<'a> {
    fn from_message(topic: &'a str, value: &'a [u8]) -> Result<Self, ()> {
        let path = topic.strip_prefix('/').unwrap_or(topic);

        if path == "list" {
            Ok(Command::List)
        } else {
            match path.strip_prefix("settings") {
                Some(path) => {
                    if value.is_empty() {
                        Ok(Command::Get { path })
                    } else {
                        Ok(Command::Set { path, value })
                    }
                }
                _ => Err(()),
            }
        }
    }
}

struct ListCache {
    topic: String<MAX_TOPIC_LENGTH>,
    correlation_data: Option<Vec<u8, MAX_CD_LENGTH>>,
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
/// use miniconf::{MqttClient, Tree};
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
///     minimq::ConfigBuilder::new(localhost.into(), &mut buffer).keepalive_interval(60),
/// )
/// .unwrap();
/// let mut settings = Settings::default();
///
/// client
///     .handled_update(&mut settings, |path, settings, new_settings| {
///         if new_settings.foo {
///             return Err("Foo!");
///         }
///         *settings = new_settings;
///         Ok(())
///     })
///     .unwrap();
/// ```
pub struct MqttClient<'buf, Settings, Stack, Clock, Broker, const Y: usize>
where
    Settings: TreeKey<Y>,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
    Broker: minimq::Broker,
{
    mqtt: minimq::Minimq<'buf, Stack, Clock, Broker>,
    state: sm::StateMachine<sm::Context<Clock, Settings, Y>>,
    prefix: String<MAX_TOPIC_LENGTH>,
    listing_state: Option<(ListCache, Iter<Settings, Y>)>,
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
        config: minimq::ConfigBuilder<'buf, Broker>,
    ) -> Result<Self, minimq::ProtocolError> {
        // Configure a will so that we can indicate whether or not we are connected.
        let prefix = String::try_from(prefix).unwrap();
        let mut connection_topic = prefix.clone();
        connection_topic.push_str("/alive").unwrap();
        let will = minimq::Will::new(&connection_topic, b"0", &[])?
            .retained()
            .qos(QoS::AtMostOnce);

        let config = config.autodowngrade_qos().will(will)?;

        let mqtt = minimq::Minimq::new(stack, clock.clone(), config);

        let meta = Settings::metadata().separator("/");
        assert!(prefix.len() + "/settings".len() + meta.max_length <= MAX_TOPIC_LENGTH);

        Ok(Self {
            mqtt,
            state: sm::StateMachine::new(sm::Context::new(clock)),
            prefix,
            listing_state: None,
        })
    }

    fn handle_listing(&mut self) {
        let Some((cache, iter)) = &mut self.listing_state else {
            return;
        };

        while self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            // Note(unwrap): Publishing should not fail because `can_publish()` was checked before
            // attempting this publish.
            let (code, path) = iter
                .next()
                .map(|path| (ResponseCode::Continue, path.unwrap()))
                .unwrap_or((ResponseCode::Ok, String::new()));

            let props = [code.as_user_property()];
            let outgoing = Publication::new(path.as_bytes())
                .topic(&cache.topic)
                .properties(&props)
                .qos(QoS::AtLeastOnce);

            let outgoing = if let Some(cd) = &cache.correlation_data {
                outgoing.correlate(cd)
            } else {
                outgoing
            };

            let publication = match outgoing.finish() {
                Ok(response) => response,
                Err(e) => {
                    // Something went wrong. Abort the listing.
                    log::error!("Listing failed to build response: {e:?}");
                    self.listing_state.take();
                    return;
                }
            };

            // Note(unwrap) We already checked that we can publish earlier.
            self.mqtt.client().publish(publication).unwrap();

            // If we're done with listing, bail out of the loop.
            if code != ResponseCode::Continue {
                self.listing_state.take();
                break;
            }
        }
    }

    fn handle_republish(&mut self, settings: &Settings) {
        while self.mqtt.client().can_publish(QoS::AtMostOnce) {
            let Some(topic) = self.state.context_mut().republish_state.next() else {
                // If we got here, we completed iterating over the topics and published them all.
                self.state.process_event(sm::Events::Complete).unwrap();
                break;
            };

            let topic = topic.unwrap();

            let mut prefixed_topic = self.prefix.clone();
            prefixed_topic
                .push_str("/settings")
                .and_then(|_| prefixed_topic.push_str(&topic))
                .unwrap();

            // Note(unwrap): This should not fail because `can_publish()` was checked before
            // attempting this publish.
            match self.mqtt.client().publish(
                // If the topic is not present, we'll fail to serialize the setting into the
                // payload and will never publish. The iterator has already incremented, so this is
                // acceptable.
                DeferredPublication::new(|buf| settings.get_json(&topic, buf))
                    .topic(&prefixed_topic)
                    .finish()
                    .unwrap(),
            ) {
                Err(minimq::PubError::Serialization(Error::Absent(_))) => {}

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
                                .properties(&[ResponseCode::Error.as_user_property()])
                                .finish()
                                .unwrap(),
                        )
                        .unwrap();
                }
                other => other.unwrap(),
            }
        }
    }

    fn handle_subscription(&mut self) {
        log::info!("MQTT connected, subscribing to settings");

        // Note(unwrap): We construct a string with two more characters than the prefix
        // structure, so we are guaranteed to have space for storage.
        let mut settings_topic = self.prefix.clone();
        settings_topic.push_str("/settings/#").unwrap();
        let mut list_topic = self.prefix.clone();
        list_topic.push_str("/list").unwrap();

        let opts = SubscriptionOptions::default().ignore_local_messages();
        let topics = [
            TopicFilter::new(&settings_topic).options(opts),
            TopicFilter::new(&list_topic).options(opts),
        ];

        if self.mqtt.client().subscribe(&topics, &[]).is_ok() {
            self.state.process_event(sm::Events::Subscribed).unwrap();
        }
    }

    fn handle_indicating_alive(&mut self) {
        // Publish a connection status message.
        let mut connection_topic = self.prefix.clone();
        connection_topic.push_str("/alive").unwrap();

        if self
            .mqtt
            .client()
            .publish(
                Publication::new(b"1")
                    .topic(&connection_topic)
                    .retain()
                    .finish()
                    .unwrap(),
            )
            .is_ok()
        {
            self.state.process_event(sm::Events::IndicatedLife).unwrap();
        }
    }

    /// Update the MQTT interface and service the network. Pass any settings changes to the handler
    /// supplied.
    ///
    /// # Args
    /// * `handler` - A closure called with updated settings that can be used to apply current
    ///   settings or validate the configuration. Arguments are (path, old_settings, new_settings).
    ///
    /// # Returns
    /// True if the settings changed. False otherwise.
    pub fn handled_update<F, E>(
        &mut self,
        settings: &mut Settings,
        handler: F,
    ) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, Settings) -> Result<(), E>,
        E: core::fmt::Display,
    {
        if !self.mqtt.client().is_connected() {
            // Note(unwrap): It's always safe to reset.
            self.state.process_event(sm::Events::Reset).unwrap();
        }

        match *self.state.state() {
            sm::States::Initial => {
                if self.mqtt.client().is_connected() {
                    self.state.process_event(sm::Events::Connected).unwrap();
                }
            }
            sm::States::ConnectedToBroker => self.handle_indicating_alive(),
            sm::States::PendingSubscribe => self.handle_subscription(),
            sm::States::PendingRepublish => {
                if self.state.context().republish_has_timed_out() {
                    self.state
                        .process_event(sm::Events::StartRepublish)
                        .unwrap();
                }
            }
            sm::States::RepublishingSettings => self.handle_republish(settings),

            // Nothing to do in the active state.
            sm::States::Active => {}
        }

        self.handle_listing();

        // All states must handle MQTT traffic.
        self.handle_mqtt_traffic(settings, handler)
    }

    fn handle_mqtt_traffic<F, E>(
        &mut self,
        settings: &mut Settings,
        mut handler: F,
    ) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, Settings) -> Result<(), E>,
        E: core::fmt::Display,
    {
        let mut updated = false;
        let poll = self.mqtt.poll(|client, topic, message, properties| {
            let Some(path) = topic.strip_prefix(self.prefix.as_str()) else {
                log::info!("Unexpected topic prefix: {topic}");
                return;
            };

            let Ok(command) = Command::from_message(path, message) else {
                log::info!("Unknown Miniconf command: {path}");
                return;
            };

            match command {
                Command::List => {
                    if !properties
                        .into_iter()
                        .any(|prop| matches!(prop, Ok(minimq::Property::ResponseTopic(_))))
                    {
                        log::info!("Discarding `List` without `ResponseTopic`");
                        return;
                    }

                    let response = match self.listing_state {
                        Some(_) => "`List` already in progress",
                        None => {
                            match handle_listing_request(properties) {
                                Err(msg) => msg,
                                Ok(cache) => {
                                    self.listing_state
                                        .replace((cache, Settings::iter_paths_unchecked("/")));

                                    // There is no positive response sent during list commands,
                                    // instead, the response is sent as a property of the listed
                                    // elements. As such, we are now finished processing a list
                                    // command.
                                    return;
                                }
                            }
                        }
                    };

                    let props = [ResponseCode::Error.as_user_property()];
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
                    let props = [ResponseCode::Ok.as_user_property()];
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

                    if let Err(minimq::PubError::Serialization(err)) = client.publish(message) {
                        if let Ok(message) = DeferredPublication::new(|mut buf| {
                            let start = buf.len();
                            write!(buf, "{}", err).and_then(|_| Ok(start - buf.len()))
                        })
                        .properties(&[ResponseCode::Error.as_user_property()])
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

                Command::Set { path, value } => {
                    let mut new_settings = settings.clone();
                    if let Err(err) = new_settings.set_json(path, value) {
                        if let Ok(response) = DeferredPublication::new(|mut buf| {
                            let start = buf.len();
                            write!(buf, "{}", err).and_then(|_| Ok(start - buf.len()))
                        })
                        .properties(&[ResponseCode::Error.as_user_property()])
                        .reply(properties)
                        .qos(QoS::AtLeastOnce)
                        .finish()
                        {
                            client.publish(response).ok();
                        }
                        return;
                    };

                    updated = true;

                    match handler(path, settings, new_settings) {
                        Ok(_) => {
                            if let Ok(response) = Publication::new("OK".as_bytes())
                                .properties(&[ResponseCode::Ok.as_user_property()])
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
                            .properties(&[ResponseCode::Error.as_user_property()])
                            .reply(properties)
                            .qos(QoS::AtLeastOnce)
                            .finish()
                            {
                                client.publish(response).ok();
                            }
                        }
                    }
                }
            }
        });
        match poll {
            Ok(_) => Ok(updated),
            Err(minimq::Error::SessionReset) => {
                log::warn!("Session reset");
                self.state.process_event(sm::Events::Reset).unwrap();
                Ok(false)
            }
            Err(other) => Err(other),
        }
    }

    /// Update the settings from the network stack without any specific handling.
    ///
    /// # Returns
    /// True if the settings changed. False otherwise
    pub fn update(&mut self, settings: &mut Settings) -> Result<bool, minimq::Error<Stack::Error>> {
        self.handled_update(settings, |_path, settings, new_settings| {
            *settings = new_settings;
            Result::<_, &'static str>::Ok(())
        })
    }

    /// Force republication of the current settings.
    ///
    /// # Note
    /// This is intended to be used if modification of a setting had side effects that affected
    /// another setting.
    pub fn force_republish(&mut self) {
        self.state.process_event(sm::Events::StartRepublish).ok();
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum ResponseCode {
    Ok,
    Continue,
    Error,
}

impl ResponseCode {
    fn as_user_property(self) -> minimq::Property<'static> {
        let string = match self {
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

fn handle_listing_request(
    properties: &minimq::types::Properties<'_>,
) -> Result<ListCache, &'static str> {
    // If the response topic is too long, send an error
    let response_topic = properties.into_iter().response_topic().unwrap();

    // If there is a CD and it's too long, send an error response.
    let correlation_data = if let Some(cd) = properties.into_iter().find_map(|prop| {
        if let Ok(minimq::Property::CorrelationData(cd)) = prop {
            Some(cd.0)
        } else {
            None
        }
    }) {
        Some(Vec::try_from(cd).map_err(|_| "Correlation data too long")?)
    } else {
        None
    };

    Ok(ListCache {
        topic: String::try_from(response_topic).map_err(|_| "Response topic too long")?,
        correlation_data,
    })
}
