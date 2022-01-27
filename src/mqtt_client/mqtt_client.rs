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
use serde_json_core::heapless::String;

use minimq::embedded_nal::{IpAddr, TcpClientStack};

use super::messages::{MqttMessage, SettingsResponse};
use crate::Miniconf;
use log::info;
use minimq::{embedded_time, QoS, Retain};

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

mod sm {
    use minimq::embedded_time::{self, duration::Extensions, Instant};
    use smlang::statemachine;

    statemachine! {
        transitions: {
            *Initial + Connected = ConnectedToBroker,
            ConnectedToBroker + Subscribed = IndicatingLife,
            IndicatingLife + IndicatedAlive / start_republish_timeout = PendingRepublish,
            PendingRepublish + MessageReceived / start_republish_timeout = PendingRepublish,
            PendingRepublish + RepublishTimeout / start_republish = RepublishingSettings,
            RepublishingSettings + RepublishComplete = Active,

            // All states transition back to `initial` on reset.
            Initial + Reset = Initial,
            ConnectedToBroker + Reset = Initial,
            IndicatingLife + Reset = Initial,
            PendingRepublish + Reset = Initial,
            RepublishingSettings + Reset = Initial,
            Active + Reset = Initial,
        }
    }

    pub struct Context<C: embedded_time::Clock> {
        clock: C,
        timeout: Option<Instant<C>>,
        pub republish_state: [usize; super::MAX_RECURSION_DEPTH],
    }

    impl<C: embedded_time::Clock> Context<C> {
        pub fn new(clock: C) -> Self {
            Self {
                clock,
                timeout: None,
                republish_state: [0; super::MAX_RECURSION_DEPTH],
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

    impl<C: embedded_time::Clock> StateMachineContext for Context<C> {
        fn start_republish_timeout(&mut self) {
            self.timeout.replace(
                self.clock.try_now().unwrap() + super::REPUBLISH_TIMEOUT_SECONDS.seconds(),
            );
        }

        fn start_republish(&mut self) {
            self.republish_state = [0; super::MAX_RECURSION_DEPTH];
        }
    }
}

/// MQTT settings interface.
pub struct MqttClient<Settings, Stack, Clock, const MESSAGE_SIZE: usize>
where
    Settings: Miniconf,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
{
    mqtt: minimq::Minimq<Stack, Clock, MESSAGE_SIZE, 1>,
    settings: Settings,
    state: sm::StateMachine<sm::Context<Clock>>,
    settings_prefix: String<MAX_TOPIC_LENGTH>,
    prefix: String<MAX_TOPIC_LENGTH>,
}

impl<Settings, Stack, Clock, const MESSAGE_SIZE: usize>
    MqttClient<Settings, Stack, Clock, MESSAGE_SIZE>
where
    Settings: Miniconf,
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

        assert!(
            settings_prefix.len() + 1 + settings.get_metadata().max_topic_size <= MAX_TOPIC_LENGTH
        );

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

        for topic in self
            .settings
            .into_iter::<MAX_TOPIC_LENGTH>(&mut self.state.context_mut().republish_state)
            .unwrap()
        {
            let mut data = [0; MESSAGE_SIZE];

            // Note(unwrap): We know this topic exists already because we just got it from the
            // iterator.
            let len = self.settings.get(&topic, &mut data).unwrap();

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
                    &[],
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
            self.state
                .process_event(sm::Events::IndicatedAlive)
                .unwrap();
        }
    }

    /// Update the MQTT interface and service the network
    ///
    /// # Returns
    /// True if the settings changed. False otherwise.
    pub fn update(&mut self) -> Result<bool, minimq::Error<Stack::Error>> {
        if !self.mqtt.client.is_connected() {
            // Note(unwrap): It's always safe to reset.
            self.state.process_event(sm::Events::Reset).unwrap();
        }

        match self.state.state() {
            &sm::States::Initial => {
                if self.mqtt.client.is_connected() {
                    self.state.process_event(sm::Events::Connected).unwrap();
                }
            }
            &sm::States::ConnectedToBroker => self.handle_subscription(),
            &sm::States::IndicatingLife => self.handle_indicating_alive(),
            &sm::States::PendingRepublish => {
                if self.state.context().republish_has_timed_out() {
                    self.state
                        .process_event(sm::Events::RepublishTimeout)
                        .unwrap();
                }
            }
            &sm::States::RepublishingSettings => self.handle_republish(),

            // Nothing to do in the active state.
            &sm::States::Active => {}
        }

        // All states must handle MQTT traffic.
        self.handle_mqtt_traffic()
    }

    fn handle_mqtt_traffic(&mut self) -> Result<bool, minimq::Error<Stack::Error>> {
        let settings = &mut self.settings;
        let mqtt = &mut self.mqtt;
        let prefix = self.settings_prefix.as_str();
        let state = &mut self.state;

        let mut response_topic: String<MAX_TOPIC_LENGTH> = String::from(self.prefix.as_str());
        response_topic.push_str("/log").unwrap();
        let default_response_topic = response_topic.as_str();

        let mut update = false;
        match mqtt.poll(|client, topic, message, properties| {
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

            log::info!("Settings update: `{}`", path);

            let message: SettingsResponse = settings
                .string_set(path.split('/').peekable(), message)
                .map(|_| {
                    update = true;
                })
                .into();

            let response = MqttMessage::new(properties, default_response_topic, &message);

            // We only care about these events in one state, so ignore the errors for the other
            // states.
            state.process_event(sm::Events::MessageReceived).ok();

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
            // If settings updated,
            Ok(_) => Ok(update),
            Err(minimq::Error::SessionReset) => {
                log::warn!("Settings MQTT session reset");
                self.state.process_event(sm::Events::Reset).unwrap();
                Ok(false)
            }
            Err(other) => Err(other),
        }
    }

    /// Get the current settings from miniconf.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}
