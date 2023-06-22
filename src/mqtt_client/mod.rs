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

        if path == "list" {
            Ok(Command::List)
        } else {
            match path.split_once('/') {
                Some(("settings", path)) => {
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
    pending_response: Option<Response<32>>,
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
            .set_keepalive_interval(KEEPALIVE_INTERVAL_SECONDS)?;

        let prefix = String::from(prefix);

        // Configure a will so that we can indicate whether or not we are connected.
        let mut connection_topic = prefix.clone();
        connection_topic.push_str("/alive").unwrap();
        mqtt.client().set_will(
            &connection_topic,
            b"0",
            QoS::AtMostOnce,
            Retain::Retained,
            &[],
        )?;

        assert!(
            prefix.len() + "/settings/".len() + Settings::metadata().max_length <= MAX_TOPIC_LENGTH
        );

        Ok(Self {
            mqtt,
            state: sm::StateMachine::new(sm::Context::new(clock)),
            settings,
            prefix,
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
            // Note(unwrap): Publishing should not fail because `can_publish()` was checked before
            // attempting this publish.
            let response: Response<MAX_TOPIC_LENGTH> = iter
                .next()
                .map(|path| Response::custom(ResponseCode::Continue, &path))
                .unwrap_or_else(Response::ok);

            let props = [minimq::Property::UserProperty(
                minimq::types::Utf8String("code"),
                minimq::types::Utf8String(response.code.as_ref()),
            )];

            self.mqtt
                .client()
                .publish(
                    // Note(unwrap): We already guaranteed that the reply properties have a response
                    // topic.
                    Publication::new(response.msg.as_bytes())
                        .reply(&reply_props)
                        .properties(&props)
                        .qos(QoS::AtLeastOnce)
                        .finish()
                        .unwrap(),
                )
                .unwrap();

            // If we're done with listing, bail out of the loop.
            if response.code != ResponseCode::Continue {
                self.listing_state.take();
                break;
            }
        }
    }

    fn handle_republish(&mut self) {
        let mut data = [0; MESSAGE_SIZE];

        while self.mqtt.client().can_publish(QoS::AtMostOnce) {
            let Some(topic) = self.state.context_mut().republish_state.next() else {
                // If we got here, we completed iterating over the topics and published them all.
                self.state.process_event(sm::Events::Complete).unwrap();
                break
            };

            // Note: The topic may be absent at runtime (`miniconf::Option` or deferred `Option`).
            let len = match self.settings.get(&topic, &mut data) {
                Err(crate::Error::PathAbsent) => continue,
                Ok(len) => len,
                e => e.unwrap(),
            };

            let mut prefixed_topic = self.prefix.clone();
            prefixed_topic
                .push_str("/settings/")
                .and_then(|_| prefixed_topic.push_str(&topic))
                .unwrap();

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
        }
    }

    fn handle_subscription(&mut self) {
        log::info!("MQTT connected, subscribing to settings");

        // Note(unwrap): We construct a string with two more characters than the prefix
        // structure, so we are guaranteed to have space for storage.
        let mut settings_topic = self.prefix.clone();
        settings_topic.push_str("/#").unwrap();

        let topic_filter = TopicFilter::new(&settings_topic)
            .options(SubscriptionOptions::default().ignore_local_messages());

        if self.mqtt.client().subscribe(&[topic_filter], &[]).is_ok() {
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
    pub fn handled_update<F, E>(&mut self, handler: F) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), E>,
        E: AsRef<str>,
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

    fn handle_mqtt_traffic<F, E>(
        &mut self,
        mut handler: F,
    ) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), E>,
        E: AsRef<str>,
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
                log::info!("Unexpected MQTT topic: {topic}");
                return;
            };

            let Ok(command) = Command::from_message(path, message) else {
                log::debug!("Unknown Miniconf command: {path}");
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

            let response: Response<32> = match command {
                Command::List => {
                    if listing_state.is_none() {
                        if properties
                            .into_iter()
                            .any(|prop| matches!(prop, Ok(minimq::Property::ResponseTopic(_))))
                        {
                            // We only reply if there is a response topic to publish the list to.
                            // Note(unwrap): The vector is guaranteed to be as large as the largest MQTT
                            // message size, so the properties (which are a portion of the message) will
                            // always fit into it.
                            properties_cache.replace(Vec::from_slice(binary_props).unwrap());
                            listing_state.replace(Default::default());
                        }
                        // Response sent with listing.
                        return;
                    }

                    Response::error("Listing in progress")
                }

                Command::Get { path } => {
                    let mut data = [0u8; MESSAGE_SIZE];
                    match settings.get(path, &mut data) {
                        Err(err) => err.into(),
                        Ok(len) => {
                            let mut topic = prefix.clone();

                            // Note(unwrap): We check that the string will fit during
                            // construction.
                            topic
                                .push_str("/settings/")
                                .and_then(|_| topic.push_str(path))
                                .unwrap();

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
                    }
                }
                Command::Set { path, value } => {
                    let mut new_settings = settings.clone();
                    match new_settings.set(path, value) {
                        Err(err) => err.into(),
                        Ok(_) => {
                            updated = true;
                            handler(path, settings, &new_settings).into()
                        }
                    }
                }
            };

            if properties
                .into_iter()
                .any(|prop| matches!(prop, Ok(minimq::Property::ResponseTopic(_))))
            {
                let props = [minimq::Property::UserProperty(
                    minimq::types::Utf8String("code"),
                    minimq::types::Utf8String(response.code.as_ref()),
                )];

                let Ok(response_pub) = minimq::Publication::new(response.msg.as_bytes())
                            .reply(properties)
                            .properties(&props)
                            .qos(QoS::AtLeastOnce)
                            .finish() else {
                log::warn!("Failed to build response message");
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
struct Response<const N: usize> {
    code: ResponseCode,
    msg: String<N>,
}

impl<const N: usize> Response<N> {
    pub fn ok() -> Self {
        Self {
            msg: String::from("OK"),
            code: ResponseCode::Ok,
        }
    }

    /// Generate a custom response with any response code.
    ///
    /// # Args
    /// * `code` - The code to provide in the response.
    /// * `msg` - The message to provide in the response.
    pub fn custom(code: ResponseCode, message: &str) -> Self {
        // Truncate the provided message to ensure it fits within the heapless String.
        Self {
            code,
            msg: String::from(&message[..N.min(message.len())]),
        }
    }

    /// Generate an error response
    ///
    /// # Args
    /// * `message` - A message to provide in the response. Will be truncated to fit.
    pub fn error(message: &str) -> Self {
        Self::custom(ResponseCode::Error, message)
    }
}

impl<T, E: AsRef<str>, const N: usize> From<Result<T, E>> for Response<N> {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(_) => Response::ok(),

            Err(error) => {
                let mut msg = String::new();
                if msg.push_str(error.as_ref()).is_err() {
                    msg = String::from("Configuration Error");
                }

                Self {
                    code: ResponseCode::Error,
                    msg,
                }
            }
        }
    }
}

impl<const N: usize> From<crate::Error> for Response<N> {
    fn from(err: crate::Error) -> Self {
        let mut msg = String::new();
        if write!(&mut msg, "{:?}", err).is_err() {
            msg = String::from("Configuration Error");
        }

        Self {
            code: ResponseCode::Error,
            msg,
        }
    }
}
