mod messages;
mod mqtt_client;
pub use mqtt_client::MqttClient;

#[non_exhaustive]
pub enum HandlerResult {
    /// A settings updated was accepted.
    UpdateAccepted,

    /// The application of a setting had ramifications on other settings values. In this case, all
    /// settings will be republished.
    UpdateSideEffects,
}

impl From<&()> for HandlerResult {
    fn from(_: &()) -> Self {
        HandlerResult::UpdateAccepted
    }
}
