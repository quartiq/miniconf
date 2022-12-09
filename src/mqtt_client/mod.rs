use serde::Serialize;
use serde_json_core::heapless::{String, Vec};

use crate::Miniconf;
use log::info;
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
    Get,
    List,
    Modify(&'a str),
}

impl<'a> Command<'a> {
    fn from_str(string: &'a str) -> Result<Self, ()> {
        let path = string.strip_prefix('/').unwrap_or(string);
        let parsed = path
            .split_once('/')
            .map(|(head, tail)| (head, Some(tail)))
            .unwrap_or((path, None));

        let command = match parsed {
            ("get", None) => Command::Get,
            ("list", None) => Command::List,
            ("settings", Some(path)) => Command::Modify(path),
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
    listing_state: Option<(Vec<u8, MESSAGE_SIZE>, MiniconfIter<Settings>)>,
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
        })
    }

    fn handle_listing(&mut self) {
        let Some((ref props, ref mut republish_state)) = &mut self.listing_state else {
            return;
        };

        let props = minimq::types::Properties::DataBlock(props);

        if !self.mqtt.client().can_publish(QoS::AtLeastOnce) {
            return;
        }

        for path in republish_state {
            // Note(unwrap): This should not fail because `can_publish()` was checked before
            // attempting this publish.
            self.mqtt
                .client()
                .publish(
                    // Note(unwrap): We already guaranteed that the reply properties have a response
                    // topic.
                    Publication::new(path.as_bytes())
                        .reply(&props)
                        .qos(QoS::AtLeastOnce)
                        .finish()
                        .unwrap(),
                )
                .unwrap();

            // If we can't publish any more messages, bail out now to prevent the iterator from
            // progressing. If we don't bail out now, we'd silently drop a setting.
            if !self.mqtt.client().can_publish(QoS::AtLeastOnce) {
                return;
            }
        }

        // Publish an empty message to denote that the listing action has completed.
        self.mqtt
            .client()
            .publish(
                // Note(unwrap): We already guaranteed that the reply properties have a response
                // topic.
                Publication::new(b"")
                    .reply(&props)
                    .qos(QoS::AtLeastOnce)
                    .finish()
                    .unwrap(),
            )
            .unwrap();

        // If we got here, we completed iterating over the topics and published them all.
        self.listing_state.take();
    }

    fn handle_republish(&mut self) {
        if !self.mqtt.client().can_publish(QoS::AtMostOnce) {
            return;
        }

        for topic in &mut self.state.context_mut().republish_state {
            let mut data = [0; MESSAGE_SIZE];

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
    pub fn handled_update<F, E>(&mut self, handler: F) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), E>,
        E: AsRef<str>,
    {
        if !self.mqtt.client().is_connected() {
            // Note(unwrap): It's always safe to reset.
            self.state.process_event(sm::Events::Reset).unwrap();
        } else {
            self.state.process_event(sm::Events::Connected).ok();
        }

        match *self.state.state() {
            sm::States::Initial => {}
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

        if self.listing_state.is_some() {
            self.handle_listing();
        }

        // All states must handle MQTT traffic.
        self.handle_mqtt_traffic(handler)
    }

    fn handle_mqtt_traffic<F, E>(
        &mut self,
        mut handler: F,
    ) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), E>,
        E: AsRef<str>,
    {
        let settings = &mut self.settings;
        let mqtt = &mut self.mqtt;
        let prefix = self.prefix.as_str();
        let listing_state = &mut self.listing_state;

        let mut updated = false;
        match mqtt.poll(|client, topic, message, properties| {
            let Some(path) = topic.strip_prefix(prefix) else {
                info!("Unexpected MQTT topic: {}", topic);
                return;
            };

            let Ok(command) = Command::from_str(path) else {
                info!("Unknown Miniconf command: {path}");
                return;
            };

            let mut data = [0u8; MESSAGE_SIZE];
            let message = match command {
                Command::Get => {
                    let path = core::str::from_utf8(message).unwrap_or("");

                    // Get a specific settings value with the provided path.
                    settings
                        .get(path, &mut data)
                        .map(|len| &data[..len])
                        .unwrap_or(b"Invalid path")
                }

                Command::List => {
                    if !properties
                        .into_iter()
                        .any(|prop| matches!(prop, Ok(minimq::Property::ResponseTopic(_))))
                    {
                        // If there's no response topic, there's no where we can publish the list.
                        // Ignore the request.
                        return;
                    }

                    let minimq::types::Properties::DataBlock(binary_props) = properties else {
                        // Received properties are always serialized, so this path should never be
                        // executed.
                        unreachable!();
                    };

                    // Note(unwrap): The vector is guaranteed to be as large as the largest MQTT
                    // message size, so the properties (which are a portion of the message) will
                    // always fit into it.
                    listing_state
                        .replace((Vec::from_slice(binary_props).unwrap(), Default::default()));
                    return;
                }

                Command::Modify(path) => {
                    let mut new_settings = settings.clone();
                    let message: SettingsResponse = match new_settings.set(path, message) {
                        Ok(_) => {
                            updated = true;
                            handler(path, settings, &new_settings).into()
                        }
                        err => {
                            let mut msg = String::new();
                            if write!(&mut msg, "{:?}", err).is_err() {
                                msg = String::from("Configuration Error");
                            }

                            SettingsResponse::error(msg)
                        }
                    };

                    match serde_json_core::to_slice(&message, &mut data) {
                        Ok(len) => &data[..len],

                        // If the message buffer is too small, we can't ever provide a response.
                        _ => return,
                    }
                }
            };

            minimq::Publication::new(message)
                .reply(properties)
                .qos(QoS::AtLeastOnce)
                .finish()
                .ok()
                .and_then(|message| client.publish(message).ok());
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

/// The payload of the MQTT response message to a settings update request.
#[derive(Serialize)]
pub struct SettingsResponse {
    code: u8,
    msg: String<64>,
}

impl SettingsResponse {
    pub fn ok() -> Self {
        Self {
            msg: String::from("OK"),
            code: 0,
        }
    }

    pub fn error(msg: String<64>) -> Self {
        Self { code: 255, msg }
    }
}

impl<T, E: AsRef<str>> From<Result<T, E>> for SettingsResponse {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(_) => SettingsResponse::ok(),

            Err(error) => {
                let mut msg = String::new();
                if msg.push_str(error.as_ref()).is_err() {
                    msg = String::from("Configuration Error");
                }

                Self::error(msg)
            }
        }
    }
}
