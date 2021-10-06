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

/// MQTT settings interface.
pub struct MqttClient<Settings, Stack, const MESSAGE_SIZE: usize>
where
    Settings: Miniconf + Default,
    Stack: TcpClientStack,
{
    default_response_topic: String<128>,
    mqtt: minimq::Minimq<Stack, MESSAGE_SIZE>,
    settings: Settings,
    subscribed: bool,
    settings_prefix: String<64>,
}

impl<Settings, Stack, const MESSAGE_SIZE: usize> MqttClient<Settings, Stack, MESSAGE_SIZE>
where
    Settings: Miniconf + Default,
    Stack: TcpClientStack,
{
    /// Construct a new MQTT settings interface.
    ///
    /// # Args
    /// * `stack` - The network stack to use for communication.
    /// * `client_id` - The ID of the MQTT client. May be an empty string for auto-assigning.
    /// * `prefix` - The MQTT device prefix to use for this device.
    /// * `broker` - The IP address of the MQTT broker to use.
    pub fn new(
        stack: Stack,
        client_id: &str,
        prefix: &str,
        broker: IpAddr,
    ) -> Result<Self, minimq::Error<Stack::Error>> {
        let mqtt = minimq::Minimq::new(broker, client_id, stack)?;

        let mut response_topic: String<128> = String::from(prefix);
        response_topic.push_str("/log").unwrap();

        let mut settings_prefix: String<64> = String::from(prefix);
        settings_prefix.push_str("/settings").unwrap();

        Ok(Self {
            mqtt,
            settings: Settings::default(),
            settings_prefix,
            default_response_topic: response_topic,
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
        if !self.subscribed && self.mqtt.client.is_connected()? {
            log::info!("MQTT connected, subscribing to settings");
            // Note(unwrap): We construct a string with two more characters than the prefix
            // strucutre, so we are guaranteed to have space for storage.
            let mut settings_topic: String<66> = String::from(self.settings_prefix.as_str());
            settings_topic.push_str("/#").unwrap();

            // We do not currently handle or process potential subscription failures. Instead, this
            // failure will be logged through the stabilizer logging interface.
            self.mqtt.client.subscribe(&settings_topic, &[]).unwrap();
            self.subscribed = true;
        }

        // Handle any MQTT traffic.
        let settings = &mut self.settings;
        let mqtt = &mut self.mqtt;
        let prefix = self.settings_prefix.as_str();
        let default_response_topic = self.default_response_topic.as_str();

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
                    minimq::QoS::AtMostOnce,
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
