use super::{Error, Miniconf};
use core::fmt::Write;
use heapless::{consts, String};

/// Represents an incoming MQTT packet.
pub struct Message<'a> {
    /// The incoming binary data composing the message.
    pub data: &'a [u8],
    /// The requeseted response topic for this message.
    pub response_topic: Option<&'a str>,

    /// The requested correlation data associated with this message.
    pub correlation_data: Option<&'a [u8]>,
}

/// Represents a response to an incoming `Message`.
pub struct Response<'a> {
    /// The topic that the response should be published on.
    pub topic: &'a str,

    /// The data associated with the response.
    pub data: String<consts::U64>,

    /// The correlation data associated with the response.
    pub correlation_data: Option<&'a [u8]>,
}


#[cfg(feature = "minimq-support")]
impl<'a> Message<'a> {
    pub fn from(data: &'a [u8], properties: &[minimq::Property<'a>]) -> Message<'a> {
        // Find correlation-data and response topics.
        let correlation_data = properties.iter().find_map(|prop| {
            if let minimq::Property::CorrelationData(data) = prop {
                Some(*data)
            } else {
                None
            }
        });
        let response_topic = properties.iter().find_map(|prop| {
            if let minimq::Property::ResponseTopic(topic) = prop {
                Some(*topic)
            } else {
                None
            }
        });

        Message {
            data,
            response_topic,
            correlation_data,
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
pub struct MiniconfInterface<T: Miniconf> {
    pub settings: T,

    settings_topic: String<consts::U128>,
    default_response_topic: String<consts::U128>,
    id: String<consts::U128>,
}

impl<T: Miniconf> MiniconfInterface<T> {
    /// Construct a new settings interface.
    ///
    /// # Args
    /// * `id` - The ID for uniquely identifying the device.
    /// * `settings` - The initial settings of the interface.
    ///
    /// # Returns
    /// A new `MqttInterface` object that can be used for settings configuration.
    pub fn new(id: &str, settings: T) -> Result<Self, Error> {
        let settings_topic = generate_topic(id, "settings/#").or(Err(Error::IdTooLong))?;
        let default_response_topic = generate_topic(id, "log").or(Err(Error::IdTooLong))?;

        Ok(Self {
            settings,

            settings_topic,
            default_response_topic,

            // Note(unwrap): We can safely assume the ID is less than our storage size, since we
            // generate longer strings above.
            id: String::from(id),
        })
    }

    /// Get the MQTT topic that should be subscribed to.
    pub fn get_listening_topic(&self) -> &str {
        &self.settings_topic
    }

    /// Update settings based on inbound MQTT traffic.
    ///
    /// #Note:
    /// This should be called whenever an incoming MQTT message is received.
    ///
    /// # Args
    /// * `topic` - The incoming message topic.
    /// * `message` - The incoming MQTT message.
    ///
    /// # Returns
    /// True if settings were updated.
    pub fn process<'msg, 's>(
        &'s mut self,
        topic: &str,
        message: Message<'msg>,
    ) -> Option<Response<'msg>>
    where
        's: 'msg,
    {
        // Check that the topic should be processed by this driver. Ignore anything that is not
        // routed to us.
        let tail = topic.strip_prefix(self.id.as_str())?;

        let mut split = tail[1..].split('/');
        let response = match split.next() {
            Some("settings") => {
                // Update the setting
                match self.settings.string_set(split.peekable(), message.data) {
                    Ok(_) => {
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
        };

        Some(Response {
            topic: message
                .response_topic
                .unwrap_or(&self.default_response_topic),
            correlation_data: message.correlation_data,
            data: response,
        })
    }
}
