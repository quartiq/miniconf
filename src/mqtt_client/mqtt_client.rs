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

/// MQTT settings interface.
pub struct MqttClient<Settings, Stack, Clock, const MESSAGE_SIZE: usize, const MESSAGE_COUNT: usize>
where
    Settings: Miniconf + Default,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
{
    mqtt: minimq::Minimq<Stack, Clock, MESSAGE_SIZE, MESSAGE_COUNT>,
    settings: Settings,
    subscribed: bool,
    settings_prefix: String<64>,
    prefix: String<64>,
}

impl<Settings, Stack, Clock, const MESSAGE_SIZE: usize, const MESSAGE_COUNT: usize>
    MqttClient<Settings, Stack, Clock, MESSAGE_SIZE, MESSAGE_COUNT>
where
    Settings: Miniconf + Default,
    Stack: TcpClientStack,
    Clock: embedded_time::Clock,
{
    /// Construct a new MQTT settings interface.
    ///
    /// # Args
    /// * `stack` - The network stack to use for communication.
    /// * `client_id` - The ID of the MQTT client. May be an empty string for auto-assigning.
    /// * `prefix` - The MQTT device prefix to use for this device.
    /// * `broker` - The IP address of the MQTT broker to use.
    /// * `clock` - The clock for managing the MQTT connection.
    pub fn new(
        stack: Stack,
        client_id: &str,
        prefix: &str,
        broker: IpAddr,
        clock: Clock,
    ) -> Result<Self, minimq::Error<Stack::Error>> {
        let mut mqtt = minimq::Minimq::new(broker, client_id, stack, clock)?;

        // Configure a will so that we can indicate whether or not we are connected.
        let mut connection_topic: String<75> = String::from(prefix);
        connection_topic.push_str("/connected").unwrap();
        mqtt.client
            .set_will(
                &connection_topic,
                "0".as_bytes(),
                QoS::AtMostOnce,
                Retain::Retained,
                &[],
            )
            .unwrap();

        let mut settings_prefix: String<64> = String::from(prefix);
        settings_prefix.push_str("/settings").unwrap();

        Ok(Self {
            mqtt,
            settings: Settings::default(),
            settings_prefix,
            prefix: String::from(prefix),
            subscribed: false,
        })
    }

    /// Update the MQTT interface and service the network
    ///
    /// # Returns
    /// True if the settings changed. False otherwise.
    pub fn update(&mut self) -> Result<bool, minimq::Error<Stack::Error>> {
        // If we're no longer subscribed to the settings topic, but we are connected to the broker,
        // resubscribe.
        if !self.subscribed && self.mqtt.client.is_connected() {
            log::info!("MQTT connected, subscribing to settings");
            // Note(unwrap): We construct a string with two more characters than the prefix
            // strucutre, so we are guaranteed to have space for storage.
            let mut settings_topic: String<66> = String::from(self.settings_prefix.as_str());
            settings_topic.push_str("/#").unwrap();

            // We do not currently handle or process potential subscription failures. Instead, this
            // failure will be logged through the stabilizer logging interface.
            self.mqtt.client.subscribe(&settings_topic, &[]).unwrap();
            self.subscribed = true;

            // Publish a connection status message.
            let mut connection_topic: String<75> = String::from(self.prefix.as_str());
            connection_topic.push_str("/connected").unwrap();
            self.mqtt
                .client
                .publish(
                    &connection_topic,
                    "1".as_bytes(),
                    QoS::AtMostOnce,
                    Retain::Retained,
                    &[],
                )
                .unwrap();
        }

        // Handle any MQTT traffic.
        let settings = &mut self.settings;
        let mqtt = &mut self.mqtt;
        let prefix = self.settings_prefix.as_str();

        let mut response_topic: String<70> = String::from(self.prefix.as_str());
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
                self.subscribed = false;
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
