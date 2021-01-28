use minimq::{
    embedded_nal::{TcpStack, IpAddr, Ipv4Addr},
    QoS, Property, MqttClient
};
use heapless::{String, consts};
use core::fmt::Write;
use super::StringSet;

#[derive(Debug)]
pub enum Error<S: TcpStack> where S::Error: core::fmt::Debug {
    /// The provided device ID is too long.
    IdTooLong,

    /// MQTT encountered an internal error.
    Mqtt(minimq::Error<S::Error>),

    /// The network stack encountered an error.
    Network(S::Error)
}

impl<S: TcpStack> From<minimq::Error<S::Error>> for Error<S> {
    fn from(err: minimq::Error<S::Error>) -> Self {
        match err {
            minimq::Error::Network(err) => Error::Network(err),
            other => Error::Mqtt(other),
        }
    }
}

/// An action that applications should act upon.
#[derive(PartialEq)]
pub enum Action {
    /// There is nothing to do except continue normal execution.
    Continue,

    /// The settings are being commit to memory. The application should take steps to make staged
    /// settings active.
    CommitSettings,
}

// Generate an MQTT topic of the form `<device_id>/<topic>`.
//
// # Returns
// The string - otherwise, an error indicating the generated string was too long.
fn generate_topic<'a, 'b>(device_id: &'a str, topic: &'b str) -> Result<String<minimq::consts::U128>, ()> {
    let mut string: String<consts::U128> = String::new();
    write!(&mut string, "{}/{}", device_id, topic).or(Err(()))?;
    Ok(string)
}

/// An interface for managing MQTT settings.
pub struct MqttInterface<T, S>
where
    T: StringSet,
    S: TcpStack,
{
    // TODO: Allow the user to specify buffer size.
    client: Option<MqttClient<minimq::consts::U256, S>>,
    pub settings: T,

    subscribed: bool,
    settings_topic: String<minimq::consts::U128>,
    commit_topic: String<minimq::consts::U128>,
    default_response_topic: String<minimq::consts::U128>,
}

impl<T, S> MqttInterface<T, S>
where
    T: StringSet,
    S: TcpStack,
{
    /// Construct a new settings interface using the network stack.
    ///
    /// # Args
    /// * `stack` - The TCP network stack to use for communication.
    /// * `id` - The ID for uniquely identifying the device.
    /// * `settings` - The initial settings of the interface.
    ///
    /// # Returns
    /// A new `MqttInterface` object that can be used for settings configuration and telemtry.
    pub fn new<'a>(stack: S, id: &'a str, settings: T) -> Result<Self, Error<S>> {
        // TODO: Allow the user to specify broker IP or allow support for DNS.
        let client: MqttClient<minimq::consts::U256, _> = MqttClient::new(
                IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                id,
                stack)?;

        let settings_topic = generate_topic(id, "settings/#").or(Err(Error::IdTooLong))?;
        let commit_topic = generate_topic(id, "commit").or(Err(Error::IdTooLong))?;
        let default_response_topic = generate_topic(id, "log").or(Err(Error::IdTooLong))?;

        Ok(Self {
            client: Some(client),
            subscribed: false,
            settings,

            settings_topic,
            default_response_topic,
            commit_topic,
        })
    }

    /// Called to periodically service the MQTT telemetry interface.
    ///
    /// # Note
    /// This function should be called whenever the underlying network stack has processed incoming
    /// or outgoing data.
    ///
    /// # Returns
    /// An `Action` indicating what action should be taken by the user application.
    pub fn update(&mut self) -> Result<Action, Error<S>> {

        let mut client = self.client.take().unwrap();

        // If we are not yet subscribed to the necessary topics, subscribe now.
        if !self.subscribed && client.is_connected()? {
            client.subscribe(&self.settings_topic, &[])?;
            client.subscribe(&self.commit_topic, &[])?;
        }

        let mut result = Action::Continue;

        let result = match client.poll(|client, topic, message, properties| {
            let mut split = topic.split('/');

            // TODO: Verify topic ID against our ID.
            let _id = split.next().unwrap();

            // Process the command
            let command = split.next().unwrap();
            let response: String<consts::U512> = match command {
                "settings" => {
                    // Handle settings failures
                    let mut response: String<consts::U512> = String::new();
                    match self.settings.string_set(split.peekable(), message) {
                        Ok(_) => write!(&mut response, "{} written", topic).unwrap(),
                        Err(error) => {
                            write!(&mut response, "Settings failure: {:?}", error).unwrap();
                        }
                    };

                    response
                },
                "commit" => {
                    result = Action::CommitSettings;
                    String::from("Committing pending settings")
                },
                _ => String::from("Unknown topic"),
            };

            // Publish the response to the request over MQTT using the ResponseTopic property if
            // possible. Otherwise, default to a logging topic.
            if let Property::ResponseTopic(topic) = properties.iter().find(|&prop| {
                if let Property::ResponseTopic(_) = *prop {
                    true
                } else {
                    false
                }
            }).or(Some(&Property::ResponseTopic(&self.default_response_topic))).unwrap() {
                client.publish(topic, &response.into_bytes(), QoS::AtMostOnce, &[]).unwrap();
            }
        }) {
            Ok(_) => Ok(result),
            Err(minimq::Error::Disconnected) => {
                self.subscribed = false;
                Ok(result)
            },
            Err(other) => Err(Error::Mqtt(other)),
        };

        self.client.replace(client);

        result
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
        &mut self.client.as_mut().unwrap().network_stack
    }

    pub fn client<F, R>(&mut self, mut func: F) -> R
    where
        F: FnMut(&mut minimq::MqttClient<minimq::consts::U256, S>) -> R
    {
        let mut client = self.client.take().unwrap();
        let result = func(&mut client);
        self.client.replace(client);

        result
    }
}
