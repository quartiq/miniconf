use serde_json_core::heapless::{String, Vec};

use crate::Miniconf;
use minimq::{
    embedded_nal::{IpAddr, TcpClientStack},
    embedded_time,
    types::{SubscriptionOptions, TopicFilter},
    Publication, QoS, Retain,
};

use core::fmt::Write;

// The maximum topic length of any settings path.
const MAX_TOPIC_LENGTH: usize = 128;

// The keepalive interval to use for MQTT in seconds.
const KEEPALIVE_INTERVAL_SECONDS: u16 = 60;

// The maximum recursive depth of a settings structure.
const MAX_RECURSION_DEPTH: usize = 8;

// The delay after not receiving messages after initial connection that settings will be
// republished.
const REPUBLISH_TIMEOUT_SECONDS: u32 = 2;

type MiniconfIter<M> = crate::MiniconfIter<M, MAX_RECURSION_DEPTH, MAX_TOPIC_LENGTH>;

mod sm {
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

    pub struct Context<C: embedded_time::Clock, M: super::Miniconf + ?Sized> {
        clock: C,
        timeout: Option<Instant<C>>,
        pub republish_state: super::MiniconfIter<M>,
    }

    impl<C: embedded_time::Clock, M: super::Miniconf> Context<C, M> {
        pub fn new(clock: C) -> Self {
            Self {
                clock,
                timeout: None,
                republish_state: Default::default(),
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

    impl<C: embedded_time::Clock, M: super::Miniconf> StateMachineContext for Context<C, M> {
        fn start_republish_timeout(&mut self) {
            self.timeout.replace(
                self.clock.try_now().unwrap() + super::REPUBLISH_TIMEOUT_SECONDS.seconds(),
            );
        }

        fn start_republish(&mut self) {
            self.republish_state = Default::default();
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
        let parsed = path
            .split_once('/')
            .map(|(head, tail)| (head, Some(tail)))
            .unwrap_or((path, None));

        let command = match parsed {
            ("list", None) => Command::List,
            ("settings", Some(path)) => {
                if value.is_empty() {
                    Command::Get { path }
                } else {
                    Command::Set { path, value }
                }
            }
            _ => return Err(()),
        };

        Ok(command)
    }
}

/// MQTT settings interface.
///
/// # Design
/// The MQTT client places the [Miniconf] paths `<path>` at the MQTT `<prefix>/settings/<path>` topic,
/// where `<prefix>` is provided in the client constructor.
///
/// It publishes its alive-ness as a `1` to `<prefix>/alive` and sets a will to publish `0` there when
/// it is disconnected.
///
/// # Limitations
/// The MQTT client logs failures to subscribe to the settings topic, but does not re-attempt to
/// connect to it when errors occur.
///
/// The client only supports paths up to 128 byte length and maximum depth of 8.
/// Keepalive interval and re-publication timeout are fixed to 60 and 2 seconds respectively.
///
/// # Example
/// ```
/// use miniconf::{MqttClient, Miniconf};
///
/// #[derive(Miniconf, Clone, Default)]
/// struct Settings {
///     foo: bool,
/// }
///
/// let mut client: MqttClient<Settings, _, _, 256> = MqttClient::new(
///     std_embedded_nal::Stack::default(),
///     "",  // client_id auto-assign
///     "quartiq/application/12345",  // prefix
///     "127.0.0.1".parse().unwrap(),
///     std_embedded_time::StandardClock::default(),
///     Settings::default(),
/// )
/// .unwrap();
///
/// client.handled_update(|path, old_settings, new_settings| {
///     if new_settings.foo {
///         return Err("Foo!");
///     }
///     *old_settings = new_settings.clone();
///     Ok(())
/// }).unwrap();
/// ```
pub struct MqttClient<Settings, Stack, Clock, const MESSAGE_SIZE: usize>
where
    Settings: Miniconf + Clone,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
{
    mqtt: minimq::Minimq<Stack, Clock, MESSAGE_SIZE, 1>,
    settings: Settings,
    state: sm::StateMachine<sm::Context<Clock, Settings>>,
    prefix: String<MAX_TOPIC_LENGTH>,
    listing_state: Option<MiniconfIter<Settings>>,
    properties_cache: Option<Vec<u8, MESSAGE_SIZE>>,
    pending_response: Option<Response>,
}

impl<Settings, Stack, Clock, const MESSAGE_SIZE: usize>
    MqttClient<Settings, Stack, Clock, MESSAGE_SIZE>
where
    Settings: Miniconf + Clone,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock + Clone,
{
    /// Construct a new MQTT settings interface.
    ///
    /// # Args
    /// * `stack` - The network stack to use for communication.
    /// * `client_id` - The ID of the MQTT client. May be an empty string for auto-assigning.
    /// * `prefix` - The MQTT device prefix to use for this device.
    /// * `broker` - The IP address of the MQTT broker to use.
    /// * `clock` - The clock for managing the MQTT connection.
    /// * `settings` - The initial settings values.
    pub fn new(
        stack: Stack,
        client_id: &str,
        prefix: &str,
        broker: IpAddr,
        clock: Clock,
        settings: Settings,
    ) -> Result<Self, minimq::Error<Stack::Error>> {
        let mut mqtt = minimq::Minimq::new(broker, client_id, stack, clock.clone())?;

        // Note(unwrap): The client was just created, so it's valid to set a keepalive interval
        // now, since we're not yet connected to the broker.
        mqtt.client()
            .set_keepalive_interval(KEEPALIVE_INTERVAL_SECONDS)
            .unwrap();

        // Configure a will so that we can indicate whether or not we are connected.
        let mut connection_topic: String<MAX_TOPIC_LENGTH> = String::from(prefix);
        connection_topic.push_str("/alive").unwrap();
        mqtt.client()
            .set_will(
                &connection_topic,
                "0".as_bytes(),
                QoS::AtMostOnce,
                Retain::Retained,
                &[],
            )
            .unwrap();

        let mut settings_prefix: String<MAX_TOPIC_LENGTH> = String::from(prefix);
        settings_prefix.push_str("/settings").unwrap();

        assert!(settings_prefix.len() + 1 + Settings::metadata().max_length <= MAX_TOPIC_LENGTH);

        Ok(Self {
            mqtt,
            state: sm::StateMachine::new(sm::Context::new(clock)),
            settings,
            prefix: String::from(prefix),
            listing_state: None,
            properties_cache: None,
            pending_response: None,
        })
    }

    fn handle_listing(&mut self) {
        let Some(ref mut iter) = &mut self.listing_state else {
            return;
        };

        let Some(ref props) = self.properties_cache else {
            return
        };

        let reply_props = minimq::types::Properties::DataBlock(props);

        while self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            let path = iter.next().unwrap_or(String::new());
            let code = if path.is_empty() {
                ResponseCode::Ok
            } else {
                ResponseCode::Continue
            };

            let props = [minimq::Property::UserProperty(
                minimq::types::Utf8String("code"),
                minimq::types::Utf8String(code.as_ref()),
            )];

            // Note(unwrap): Publishing should not fail because `can_publish()` was checked before
            // attempting this publish.
            self.mqtt
                .client()
                .publish(
                    // Note(unwrap): We already guaranteed that the reply properties have a response
                    // topic.
                    Publication::new(path.as_bytes())
                        .reply(&reply_props)
                        .properties(&props)
                        .qos(QoS::AtLeastOnce)
                        .finish()
                        .unwrap(),
                )
                .unwrap();

            // If we're done with listing, bail out of the loop.
            if code != ResponseCode::Continue {
                self.listing_state.take();
                break;
            }
        }
    }

    fn handle_republish(&mut self) {
        if !self.mqtt.client().can_publish(QoS::AtMostOnce) {
            return;
        }

        let mut data = [0; MESSAGE_SIZE];
        for topic in &mut self.state.context_mut().republish_state {
            // Note: The topic may be absent at runtime (`miniconf::Option` or deferred `Option`).
            let len = match self.settings.get(&topic, &mut data) {
                Err(crate::Error::PathAbsent) => continue,
                Ok(len) => len,
                e => e.unwrap(),
            };

            let mut prefixed_topic: String<MAX_TOPIC_LENGTH> = String::new();
            write!(&mut prefixed_topic, "{}/settings/{}", &self.prefix, &topic).unwrap();

            // Note(unwrap): This should not fail because `can_publish()` was checked before
            // attempting this publish.
            self.mqtt
                .client()
                .publish(
                    Publication::new(&data[..len])
                        .topic(&prefixed_topic)
                        .finish()
                        .unwrap(),
                )
                .unwrap();

            // If we can't publish any more messages, bail out now to prevent the iterator from
            // progressing. If we don't bail out now, we'd silently drop a setting.
            if !self.mqtt.client().can_publish(QoS::AtMostOnce) {
                return;
            }
        }

        // If we got here, we completed iterating over the topics and published them all.
        self.state.process_event(sm::Events::Complete).unwrap();
    }

    fn handle_subscription(&mut self) {
        log::info!("MQTT connected, subscribing to settings");

        // Note(unwrap): We construct a string with two more characters than the prefix
        // structure, so we are guaranteed to have space for storage.
        let mut settings_topic: String<MAX_TOPIC_LENGTH> = String::from(self.prefix.as_str());
        settings_topic.push_str("/#").unwrap();

        let topic_filter = TopicFilter::new(&settings_topic)
            .options(SubscriptionOptions::default().ignore_local_messages());

        if self.mqtt.client().subscribe(&[topic_filter], &[]).is_ok() {
            self.state.process_event(sm::Events::Subscribed).unwrap();
        }
    }

    fn handle_indicating_alive(&mut self) {
        // Publish a connection status message.
        let mut connection_topic: String<MAX_TOPIC_LENGTH> = String::from(self.prefix.as_str());
        connection_topic.push_str("/alive").unwrap();

        if self
            .mqtt
            .client()
            .publish(
                Publication::new("1".as_bytes())
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
    pub fn handled_update<F>(&mut self, handler: F) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), &'static str>,
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
            sm::States::RepublishingSettings => self.handle_republish(),

            // Nothing to do in the active state.
            sm::States::Active => {}
        }

        self.handle_listing();

        self.handle_pending_response()?;

        // All states must handle MQTT traffic.
        self.handle_mqtt_traffic(handler)
    }

    fn handle_pending_response(&mut self) -> Result<(), minimq::Error<Stack::Error>> {
        // Try to publish any pending response.
        if !self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            return Ok(());
        }

        let Some(response) = self.pending_response.take() else {
            return Ok(());
        };

        let Some(props) = self.properties_cache.as_ref() else {
            return Ok(());
        };

        let reply_props = minimq::types::Properties::DataBlock(props);

        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("code"),
            minimq::types::Utf8String(response.code.as_ref()),
        )];

        let Ok(response) = minimq::Publication::new(response.msg.as_bytes())
                        .reply(&reply_props)
                        .properties(&props)
                        .qos(QoS::AtLeastOnce)
                        .finish() else {
            return Ok(());
        };

        self.mqtt.client().publish(response)?;

        Ok(())
    }

    fn handle_mqtt_traffic<F>(
        &mut self,
        mut handler: F,
    ) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), &'static str>,
    {
        let Self {
            ref mut settings,
            ref mut mqtt,
            prefix,
            ref mut listing_state,
            ref mut pending_response,
            ref mut properties_cache,
            ..
        } = self;

        let mut updated = false;
        match mqtt.poll(|client, topic, message, properties| {
            let Some(path) = topic.strip_prefix(prefix.as_str()) else {
                log::info!("Unexpected MQTT topic: {}", topic);
                return;
            };

            let Ok(command) = Command::from_message(path, message) else {
                log::info!("Unknown Miniconf command: {path}");
                return;
            };

            if pending_response.is_some() {
                log::warn!("There is still a response pending, ignoring inbound traffic");
                return;
            }

            let minimq::types::Properties::DataBlock(binary_props) = properties else {
                // Received properties are always serialized, so this path should never be
                // executed.
                unreachable!();
            };

            let mut data = [0u8; MESSAGE_SIZE];
            let response: Response = match command {
                Command::List => {
                    if listing_state.is_none() {
                        if !properties
                            .into_iter()
                            .any(|prop| matches!(prop, Ok(minimq::Property::ResponseTopic(_))))
                        {
                            // If there's no response topic, there's no where we can publish the list.
                            // Ignore the request.
                            return;
                        }

                        // Note(unwrap): The vector is guaranteed to be as large as the largest MQTT
                        // message size, so the properties (which are a portion of the message) will
                        // always fit into it.
                        properties_cache.replace(Vec::from_slice(binary_props).unwrap());
                        listing_state.replace(Default::default());
                        return;
                    }

                    Response::error("Listing in progress")
                }

                Command::Get { path } => {
                    match settings.get(path, &mut data) {
                        Ok(len) => {
                            let mut topic: String<MAX_TOPIC_LENGTH> = String::new();

                            // Note(unwraps): We check that the string will fit during
                            // construction.
                            topic.push_str(prefix).unwrap();
                            topic.push_str("/settings/").unwrap();
                            topic.push_str(path).unwrap();

                            // Note(unwrap): This construction cannot fail because there's always a
                            // valid topic.
                            let message = minimq::Publication::new(&data[..len])
                                .reply(properties)
                                // Override the response topic with the path.
                                .topic(&topic)
                                .qos(QoS::AtLeastOnce)
                                .finish()
                                .unwrap();

                            if client.publish(message).is_err() {
                                Response::error("Can't publish")
                            } else {
                                Response::ok()
                            }
                        }
                        Err(err) => Response::error(err.as_str()),
                    }
                }

                Command::Set { path, value } => {
                    let mut new_settings = settings.clone();
                    match new_settings.set(path, value) {
                        Ok(_) => {
                            updated = true;
                            handler(path, settings, &new_settings).into()
                        }
                        Err(err) => Response::error(err.as_str()),
                    }
                }
            };

            let props = [minimq::Property::UserProperty(
                minimq::types::Utf8String("code"),
                minimq::types::Utf8String(response.code.as_ref()),
            )];

            let Ok(response_pub) = minimq::Publication::new(response.msg.as_bytes())
                            .reply(properties)
                            .properties(&props)
                            .qos(QoS::AtLeastOnce)
                            .finish() else {
                return;
            };

            // If we cannot publish the response yet (possibly because we just published something
            // that hasn't completed yet), cache the response for future transmission.
            if client.publish(response_pub).is_err() {
                // Note(unwrap): The vector is guaranteed to be as large as the largest MQTT
                // message size, so the properties (which are a portion of the message) will
                // always fit into it.
                properties_cache.replace(Vec::from_slice(binary_props).unwrap());
                pending_response.replace(response);
            }
        }) {
            Ok(_) => Ok(updated),
            Err(minimq::Error::SessionReset) => {
                log::warn!("Settings MQTT session reset");
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
    pub fn update(&mut self) -> Result<bool, minimq::Error<Stack::Error>> {
        self.handled_update(|_, old, new| {
            *old = new.clone();
            Result::<(), &'static str>::Ok(())
        })
    }

    /// Get the current settings from miniconf.
    pub fn settings(&self) -> &Settings {
        &self.settings
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

#[derive(PartialEq)]
enum ResponseCode {
    Ok,
    Continue,
    Error,
}

impl AsRef<str> for ResponseCode {
    fn as_ref(&self) -> &str {
        match self {
            ResponseCode::Ok => "Ok",
            ResponseCode::Continue => "Continue",
            ResponseCode::Error => "Error",
        }
    }
}

/// The payload of the MQTT response message to a settings update request.
struct Response {
    code: ResponseCode,
    msg: &'static str,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            msg: "OK",
            code: ResponseCode::Ok,
        }
    }

    /// Generate a custom response with any response code.
    ///
    /// # Args
    /// * `code` - The code to provide in the response.
    /// * `msg` - The message to provide in the response.
    pub fn custom(code: ResponseCode, msg: &'static str) -> Self {
        // Truncate the provided message to ensure it fits within the heapless String.
        Self { code, msg }
    }

    /// Generate an error response
    ///
    /// # Args
    /// * `message` - A message to provide in the response. Will be truncated to fit.
    pub fn error(message: &'static str) -> Self {
        Self::custom(ResponseCode::Error, message)
    }
}

impl<T> From<Result<T, &'static str>> for Response {
    fn from(result: Result<T, &'static str>) -> Self {
        match result {
            Ok(_) => Response::ok(),

            Err(error) => Self {
                code: ResponseCode::Error,
                msg: error,
            },
        }
    }
}

impl crate::Error {
    fn as_str(&self) -> &'static str {
        match self {
            crate::Error::PathNotFound => "PathNotFound",
            crate::Error::PathTooLong => "PathTooLong",
            crate::Error::PathTooShort => "PathTooShort",
            crate::Error::BadIndex => "BadIndex",
            crate::Error::PathAbsent => "PathAbsent",
            crate::Error::Serialization(serde_json_core::ser::Error::BufferFull) => {
                "Serialization(BufferFull)"
            }
            crate::Error::Serialization(_) => "Serialization(Unknown)",
            crate::Error::Deserialization(serde_json_core::de::Error::EofWhileParsingList) => {
                "Deserialization(EofWhileParsingList)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::EofWhileParsingObject) => {
                "Deserialization(EofWhileParsingObject)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::EofWhileParsingString) => {
                "Deserialization(EofWhileParsingString)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::EofWhileParsingNumber) => {
                "Deserialization(EofWhileParsingNumber)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::ExpectedColon) => {
                "Deserialization(ExpectedColon)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::ExpectedListCommaOrEnd) => {
                "Deserialization(ExpectedListCommaOrEnd)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::ExpectedObjectCommaOrEnd) => {
                "Deserialization(ExpectedObjectCommaOrEnd)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::ExpectedSomeIdent) => {
                "Deserialization(ExpectedSomeIdent)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::ExpectedSomeValue) => {
                "Deserialization(ExpectedSomeValue)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::InvalidNumber) => {
                "Deserialization(InvalidNumber)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::InvalidType) => {
                "Deserialization(InvalidType)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::InvalidUnicodeCodePoint) => {
                "Deserialization(InvalidUnicodeCodePoint)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::KeyMustBeAString) => {
                "Deserialization(KeyMustBeAString)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::TrailingCharacters) => {
                "Deserialization(TrailingCharacters)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::TrailingComma) => {
                "Deserialization(TrailingComma)"
            }
            crate::Error::Deserialization(serde_json_core::de::Error::CustomError) => {
                "Deserialization(CustomError)"
            }
            crate::Error::Deserialization(_) => "Deserialization(Unknown)",
        }
    }
}
