use super::StringSet;
use core::fmt::Write;
use heapless::{consts, String};
use minimq::{
    embedded_nal::{IpAddr, TcpStack},
    MqttClient, Property, QoS,
};

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
fn generate_topic<'a, 'b>(device_id: &'a str, topic: &'b str) -> Result<String<consts::U128>, ()> {
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
    settings_topic: String<consts::U128>,
    commit_topic: String<consts::U128>,
    default_response_topic: String<consts::U128>,
    id: String<consts::U128>,
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
    /// * `broker` - The IpAddress of the MQTT broker.
    /// * `settings` - The initial settings of the interface.
    ///
    /// # Returns
    /// A new `MqttInterface` object that can be used for settings configuration and telemtry.
    pub fn new<'a>(
        stack: S,
        id: &'a str,
        broker: IpAddr,
        settings: T,
    ) -> Result<Self, Error<S::Error>> {
        let client: MqttClient<minimq::consts::U256, _> = MqttClient::new(broker, id, stack)?;

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
    /// An `Action` indicating what action should be taken by the user application.
    pub fn update(&mut self) -> Result<Action, Error<S::Error>> {
        // Note(unwrap): We maintain strict control of the client object, so it should always be
        // present.
        let mut client = self.client.take().unwrap();

        // If we are not yet subscribed to the necessary topics, subscribe now.
        if !self.subscribed && client.is_connected()? {
            client.subscribe(&self.settings_topic, &[])?;
            client.subscribe(&self.commit_topic, &[])?;
            self.subscribed = true;
        }

        let mut result = Action::Continue;

        let result = match client.poll(|client, topic, message, properties| {
            let mut split = topic.split('/');

            // Publish the response to the request over MQTT using the ResponseTopic property if
            // possible. Otherwise, default to a logging topic.
            let response_topic = if let Some(Property::ResponseTopic(topic)) =
                properties.iter().find(|&prop| {
                    if let Property::ResponseTopic(_) = *prop {
                        true
                    } else {
                        false
                    }
                }) {
                *topic
            } else {
                &self.default_response_topic
            };

            // Verify topic ID against our ID.
            let id = split.next();
            if id.is_none() {
                // Make a best-effort attempt to send the response. If we get a failure, we may have
                // disconnected or the peer provided an invalid topic to respond to. Ignore the
                // failure in these cases.
                client
                    .publish(
                        response_topic,
                        "No ID speciifed".as_bytes(),
                        QoS::AtMostOnce,
                        &[],
                    )
                    .ok();
                return;
            }

            if id.unwrap() != self.id {
                let mut response: String<consts::U512> = String::new();
                write!(&mut response, "Invalid ID: {:?}", id)
                    .unwrap_or_else(|_| response = String::from("Bad ID"));

                // Make a best-effort attempt to send the response. If we get a failure, we may have
                // disconnected or the peer provided an invalid topic to respond to. Ignore the
                // failure in these cases.
                client
                    .publish(response_topic, &response.into_bytes(), QoS::AtMostOnce, &[])
                    .ok();
                return;
            }

            // Process the command
            let response = match split.next() {
                Some("settings") => {
                    // Handle settings failures
                    let mut response: String<consts::U512> = String::new();
                    match self.settings.string_set(split.peekable(), message) {
                        Ok(_) => write!(&mut response, "{} written", topic)
                            .unwrap_or_else(|_| response = String::from("Setting written")),
                        Err(error) => {
                            write!(&mut response, "Settings failure: {:?}", error)
                                .unwrap_or_else(|_| response = String::from("Settings failed"));
                        }
                    };

                    response
                }
                Some("commit") => {
                    result = Action::CommitSettings;
                    String::from("Committing pending settings")
                }
                Some(_) => String::from("Unknown topic"),
                None => String::from("No topic provided"),
            };

            // Make a best-effort attempt to send the response. If we get a failure, we may have
            // disconnected or the peer provided an invalid topic to respond to. Ignore the
            // failure in these cases.
            client
                .publish(response_topic, &response.into_bytes(), QoS::AtMostOnce, &[])
                .ok();
        }) {
            Ok(_) => Ok(result),
            Err(minimq::Error::Disconnected) => {
                self.subscribed = false;
                Ok(result)
            }
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
        F: FnMut(&mut minimq::MqttClient<minimq::consts::U256, S>) -> R,
    {
        // Note(unwrap): We maintain strict control of the client object, so it should always be
        // present.
        let mut client = self.client.take().unwrap();
        let result = func(&mut client);
        self.client.replace(client);

        result
    }
}
