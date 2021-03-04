use super::Miniconf;
use core::fmt::Write;
use heapless::{consts, String};

use minimq::{embedded_nal::TcpStack, generic_array::ArrayLength, MqttClient, Property, QoS};

#[derive(Debug)]
pub enum Error<E: core::fmt::Debug> {
    /// The provided device ID is too long.
    IdTooLong,

    /// MQTT encountered an internal error.
    Mqtt(minimq::Error<E>),

    /// The network stack encountered an error.
    Network(E),
}

impl<E: core::fmt::Debug> From<minimq::Error<E>> for Error<E> {
    fn from(err: minimq::Error<E>) -> Self {
        match err {
            minimq::Error::Network(err) => Error::Network(err),
            other => Error::Mqtt(other),
        }
    }
}

// Generate an MQTT topic of the form `<device_id>/<topic>`.
//
// # Returns
// The string - otherwise, an error indicating the generated string was too long.
fn generate_topic(device_id: &str, topic: &str) -> Result<String<consts::U128>, ()> {
    let mut string: String<consts::U128> = String::new();
    write!(&mut string, "{}/{}", device_id, topic).or(Err(()))?;
    Ok(string)
}

/// An interface for managing MQTT settings.
pub struct MqttInterface<T, S, MU>
where
    T: Miniconf,
    S: TcpStack,
    MU: ArrayLength<u8>,
{
    client: Option<MqttClient<MU, S>>,
    pub settings: T,

    subscribed: bool,
    settings_topic: String<consts::U128>,
    default_response_topic: String<consts::U128>,
    id: String<consts::U128>,
}

impl<T, S, MU> MqttInterface<T, S, MU>
where
    T: Miniconf,
    S: TcpStack,
    MU: ArrayLength<u8>,
{
    /// Construct a new settings interface using the network stack.
    ///
    /// # Args
    /// * `client` - The MQTT client to use for the interface.
    /// * `id` - The ID for uniquely identifying the device. This must conform with MQTT client-id
    ///          rules. Specifically, only alpha-numeric values are allowed.
    /// * `settings` - The initial settings of the interface.
    ///
    /// # Returns
    /// A new `MqttInterface` object that can be used for settings configuration and telemtry.
    pub fn new(client: MqttClient<MU, S>, id: &str, settings: T) -> Result<Self, Error<S::Error>> {
        let settings_topic = generate_topic(id, "settings/#").or(Err(Error::IdTooLong))?;
        let default_response_topic = generate_topic(id, "log").or(Err(Error::IdTooLong))?;

        Ok(Self {
            client: Some(client),
            subscribed: false,
            settings,

            settings_topic,
            default_response_topic,

            // Note(unwrap): We can safely assume the ID is less than our storage size, since we
            // generate longer strings above.
            id: String::from(id),
        })
    }

    /// Called to periodically service the MQTT telemetry interface.
    ///
    /// # Note
    /// This function should be called whenever the underlying network stack has processed incoming
    /// or outgoing data.
    ///
    /// # Returns
    /// True if settings were updated.
    pub fn update(&mut self) -> Result<bool, Error<S::Error>> {
        // Note(unwrap): We maintain strict control of the client object, so it should always be
        // present.
        let mut client = self.client.take().unwrap();

        let connected = match client.is_connected() {
            Ok(connected) => connected,
            Err(other) => {
                self.client.replace(client);
                return Err(other.into())
            }
        };

        // If we are not yet subscribed to the necessary topics, subscribe now.
        if !self.subscribed && connected {
            match client.subscribe(&self.settings_topic, &[]) {
                Err(error) => {
                    self.client.replace(client);
                    return Err(error.into())
                }
                Ok(_) => {}
            }
            self.subscribed = true;
        }

        // Note: Due to some oddities in minimq, we are locally caching the return value of the
        // `poll` closure into the `settings_update` variable.
        let mut settings_update = false;

        let result = match client.poll(|client, topic, message, properties| {
            let (incoming_update, response) = self.process_incoming(topic, message);
            settings_update = incoming_update;

            // Publish the response to the request over MQTT using the ResponseTopic property if
            // possible. Otherwise, default to a logging topic.
            let response_topic = if let Some(Property::ResponseTopic(topic)) = properties
                .iter()
                .find(|&prop| matches!(*prop, Property::ResponseTopic(_)))
            {
                *topic
            } else {
                &self.default_response_topic
            };

            // Make a best-effort attempt to send the response. If we get a failure, we may have
            // disconnected or the peer provided an invalid topic to respond to. Ignore the
            // failure in these cases.
            client
                .publish(response_topic, &response.into_bytes(), QoS::AtMostOnce, &[])
                .ok();
        }) {
            Ok(_) => Ok(settings_update),
            Err(minimq::Error::Disconnected) => {
                self.subscribed = false;
                Ok(settings_update)
            }
            Err(other) => Err(Error::Mqtt(other)),
        };

        self.client.replace(client);

        result
    }

    // Process an incoming MQTT message
    //
    // # Args
    // * `topic` - The provided (fully-specified) MQTT topic of the message.
    // * `message` - the raw message payload.
    //
    // # Returns
    // (update, response) - where `update` is true if settings were updated and `response` is a
    // response to transmit over the MQTT interface as a result of the message.
    fn process_incoming(&mut self, topic: &str, message: &[u8]) -> (bool, String<consts::U64>) {
        let mut update = false;

        // Verify the ID of the message by stripping the ID prefix from the received topic.
        let response = if let Some(tail) = topic.strip_prefix(self.id.as_str()) {
            // Process the command - the tail is always preceeded by a leading slash, so ignore
            // that for the purposes of getting the topic.
            let mut split = tail[1..].split('/');
            match split.next() {
                Some("settings") => {
                    // Handle settings failures
                    match self.settings.string_set(split.peekable(), message) {
                        Ok(_) => {
                            update = true;
                            let mut response: String<consts::U64> = String::new();
                            write!(&mut response, "{} written", topic)
                                .unwrap_or_else(|_| response = String::from("Setting staged"));
                            response
                        }
                        Err(error) => {
                            let mut response: String<consts::U64> = String::new();
                            write!(&mut response, "Settings failure: {:?}", error)
                                .unwrap_or_else(|_| response = String::from("Setting failed"));
                            response
                        }
                    }
                }
                Some(_) => String::from("Unknown topic"),
                None => String::from("No topic provided"),
            }
        } else {
            String::from("Invalid ID specified")
        };

        (update, response)
    }

    /// Get mutable access to the underlying network stack.
    ///
    /// # Note
    /// This function is intended to provide a means to the underlying network stack if it needs to
    /// be periodically updated or serviced.
    ///
    /// # Returns
    /// A temporary mutable reference to the underlying network stack used by MQTT.
    pub fn network_stack(&mut self) -> &mut S {
        // Note(unwrap): We maintain strict control of the client object, so it should always be
        // present.
        &mut self.client.as_mut().unwrap().network_stack
    }

    /// Get mutable access to the MQTT client.
    ///
    /// # Args
    /// * `func` - The closure that accepts the MQTT client for temporary usage.
    ///
    /// # Returns
    /// The return value provided by the closure.
    pub fn client<F, R>(&mut self, mut func: F) -> R
    where
        F: FnMut(&mut minimq::MqttClient<MU, S>) -> R,
    {
        // Note(unwrap): We maintain strict control of the client object, so it should always be
        // present.
        let mut client = self.client.take().unwrap();
        let result = func(&mut client);
        self.client.replace(client);

        result
    }
}
