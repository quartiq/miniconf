/// MQTT-based Run-time Settings Client
///
/// # Design
/// The MQTT client places all settings paths behind a `<prefix>/settings/` path prefix, where
/// `<prefix>` is provided in the client constructor. This prefix is then stripped away to get the
/// settings path for [Miniconf].
///
/// ## Example
/// With an MQTT client prefix of `dt/sinara/stabilizer` and a settings path of `adc/0/gain`, the
/// full MQTT path would be `dt/sinara/stabilizer/settings/adc/0/gain`.
///
/// # Limitations
/// The MQTT client logs failures to subscribe to the settings topic, but does not re-attempt to
/// connect to it when errors occur.
///
/// Responses to settings updates are sent without quality-of-service guarantees, so there's no
/// guarantee that the requestee will be informed that settings have been applied.
///
/// The library only supports serialized settings up to 256 bytes currently.
mod messages;

use serde_json_core::heapless::String;

use minimq::embedded_nal::{IpAddr, TcpClientStack};

use crate::Miniconf;
use log::{debug, info};
use messages::{MqttMessage, SettingsResponse};
use minimq::{embedded_time, Property, QoS, Retain};

use core::fmt::Write;

// The maximum topic length of any settings path.
const MAX_TOPIC_LENGTH: usize = 128;

// Correlation data reserved for indicating a republished message.
const REPUBLISH_CORRELATION_DATA: Property = Property::CorrelationData("REPUBLISH".as_bytes());

// The keepalive interval to use for MQTT in seconds.
const KEEPALIVE_INTERVAL_SECONDS: u16 = 60;

// The maximum recursive depth of a settings structure.
const MAX_RECURSION_DEPTH: usize = 8;

// The delay after not receiving messages after initial connection that settings will be
// republished.
const REPUBLISH_TIMEOUT_SECONDS: u32 = 2;

type MiniconfIter<M> = crate::MiniconfIter<M, MAX_RECURSION_DEPTH, MAX_TOPIC_LENGTH>;

mod sm {
    #![allow(clippy::derive_partial_eq_without_eq)]

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
            RepublishingSettings + RepublishComplete = Active,

            // All states transition back to `initial` on reset.
            Initial + Reset = Initial,
            ConnectedToBroker + Reset = Initial,
            PendingSubscribe + Reset = Initial,
            PendingRepublish + Reset = Initial,
            RepublishingSettings + Reset = Initial,
            Active + Reset = Initial,
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

/// MQTT settings interface.
pub struct MqttClient<Settings, Stack, Clock, const MESSAGE_SIZE: usize>
where
    Settings: Miniconf + Clone,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
{
    mqtt: minimq::Minimq<Stack, Clock, MESSAGE_SIZE, 1>,
    settings: Settings,
    state: sm::StateMachine<sm::Context<Clock, Settings>>,
    settings_prefix: String<MAX_TOPIC_LENGTH>,
    prefix: String<MAX_TOPIC_LENGTH>,
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
        mqtt.client
            .set_keepalive_interval(KEEPALIVE_INTERVAL_SECONDS)
            .unwrap();

        // Configure a will so that we can indicate whether or not we are connected.
        let mut connection_topic: String<MAX_TOPIC_LENGTH> = String::from(prefix);
        connection_topic.push_str("/alive").unwrap();
        mqtt.client
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
            settings_prefix,
            prefix: String::from(prefix),
        })
    }

    fn handle_republish(&mut self) {
        if !self.mqtt.client.can_publish(QoS::AtMostOnce) {
            return;
        }

        for topic in &mut self.state.context_mut().republish_state {
            let mut data = [0; MESSAGE_SIZE];

            // Note: The topic may not exist at runtime (`miniconf::Option` or deferred `Option`).
            let len = match self.settings.get(&topic, &mut data) {
                Err(crate::Error::PathNotFound) => continue,
                Ok(len) => len,
                e => e.unwrap(),
            };

            let mut prefixed_topic: String<MAX_TOPIC_LENGTH> = String::new();
            write!(&mut prefixed_topic, "{}/{}", &self.settings_prefix, &topic).unwrap();

            // Note(unwrap): This should not fail because `can_publish()` was checked before
            // attempting this publish.
            self.mqtt
                .client
                .publish(
                    &prefixed_topic,
                    &data[..len],
                    QoS::AtMostOnce,
                    Retain::NotRetained,
                    &[REPUBLISH_CORRELATION_DATA],
                )
                .unwrap();

            // If we can't publish any more messages, bail out now to prevent the iterator from
            // progressing. If we don't bail out now, we'd silently drop a setting.
            if !self.mqtt.client.can_publish(QoS::AtMostOnce) {
                return;
            }
        }

        // If we got here, we completed iterating over the topics and published them all.
        self.state
            .process_event(sm::Events::RepublishComplete)
            .unwrap();
    }

    fn handle_subscription(&mut self) {
        log::info!("MQTT connected, subscribing to settings");

        // Note(unwrap): We construct a string with two more characters than the prefix
        // structure, so we are guaranteed to have space for storage.
        let mut settings_topic: String<MAX_TOPIC_LENGTH> =
            String::from(self.settings_prefix.as_str());
        settings_topic.push_str("/#").unwrap();

        if self.mqtt.client.subscribe(&settings_topic, &[]).is_ok() {
            self.state.process_event(sm::Events::Subscribed).unwrap();
        }
    }

    fn handle_indicating_alive(&mut self) {
        // Publish a connection status message.
        let mut connection_topic: String<MAX_TOPIC_LENGTH> = String::from(self.prefix.as_str());
        connection_topic.push_str("/alive").unwrap();

        if self
            .mqtt
            .client
            .publish(
                &connection_topic,
                "1".as_bytes(),
                QoS::AtMostOnce,
                Retain::Retained,
                &[],
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
    /// # Example
    /// ```rust
    /// #[derive(miniconf::Miniconf, Clone)]
    /// struct Settings {
    ///     threshold: u32,
    /// }
    ///
    /// # let mut client: miniconf::MqttClient<Settings, _, _, 256> = miniconf::MqttClient::new(
    /// #     std_embedded_nal::Stack::default(),
    /// #     "",
    /// #     "sample/prefix",
    /// #     "127.0.0.1".parse().unwrap(),
    /// #     std_embedded_time::StandardClock::default(),
    /// #     Settings { threshold: 0 },
    /// # )
    /// # .unwrap();
    ///
    /// // let mut client = miniconf::MqttClient::new(...);
    /// client.handled_update(|path, old_settings, new_settings| {
    ///     if new_settings.threshold > 5 {
    ///         return Err("Requested threshold too high");
    ///     }
    ///
    ///     *old_settings = new_settings.clone();
    ///
    ///     Ok(())
    /// }).unwrap();
    /// ```
    ///
    /// # Returns
    /// True if the settings changed. False otherwise.
    pub fn handled_update<F, E>(&mut self, handler: F) -> Result<bool, minimq::Error<Stack::Error>>
    where
        F: FnMut(&str, &mut Settings, &Settings) -> Result<(), E>,
        E: AsRef<str>,
    {
        if !self.mqtt.client.is_connected() {
            // Note(unwrap): It's always safe to reset.
            self.state.process_event(sm::Events::Reset).unwrap();
        }

        match *self.state.state() {
            sm::States::Initial => {
                if self.mqtt.client.is_connected() {
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
        let prefix = self.settings_prefix.as_str();

        let mut response_topic: String<MAX_TOPIC_LENGTH> = String::from(self.prefix.as_str());
        response_topic.push_str("/log").unwrap();
        let default_response_topic = response_topic.as_str();

        let mut updated = false;
        match mqtt.poll(|client, topic, message, properties| {
            // If the incoming message has republish correlation data, ignore it.
            if properties
                .iter()
                .any(|&prop| prop == REPUBLISH_CORRELATION_DATA)
            {
                debug!("Ignoring republish data");
                return;
            }

            let path = match topic.strip_prefix(prefix) {
                // For paths, we do not want to include the leading slash.
                Some(path) => {
                    if !path.is_empty() {
                        &path[1..]
                    } else {
                        path
                    }
                }
                None => {
                    info!("Unexpected MQTT topic: {}", topic);
                    return;
                }
            };

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

            let response = MqttMessage::new(properties, default_response_topic, &message);

            client
                .publish(
                    response.topic,
                    &response.message,
                    // TODO: When Minimq supports more QoS levels, this should be increased to
                    // ensure that the client has received it at least once.
                    QoS::AtMostOnce,
                    Retain::NotRetained,
                    &response.properties,
                )
                .ok();
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
