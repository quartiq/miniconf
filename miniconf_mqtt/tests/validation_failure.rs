#![cfg(feature = "mqtt-client")]

use miniconf::{Deserialize, Tree};
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

const RESPONSE_TOPIC: &str = "validation_failure/device/response";

#[derive(Clone, Debug, Default, Tree)]
struct Settings {
    #[tree(validate=Self::check)]
    error: bool,
}
impl Settings {
    fn check(&self, new: bool, _ident: &str, _old: &bool) -> Result<bool, &'static str> {
        new.then_some(new).ok_or("Should exit")
    }
}

#[derive(Deserialize)]
struct Response {
    code: u8,
    _message: heapless::String<256>,
}

async fn client_task() {
    // Construct a Minimq client to the broker for publishing requests.
    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
    let mut mqtt: minimq::Minimq<'_, _, _, minimq::broker::IpBroker> = minimq::Minimq::new(
        Stack,
        StandardClock::default(),
        minimq::ConfigBuilder::new(localhost.into(), &mut buffer).keepalive_interval(60),
    );

    // Wait for the broker connection
    while !mqtt.client().is_connected() {
        mqtt.poll(|_client, _topic, _message, _properties| {})
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    let topic_filter = minimq::types::TopicFilter::new(RESPONSE_TOPIC);
    mqtt.client().subscribe(&[topic_filter], &[]).unwrap();

    // Wait the other device to connect.
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Configure the error variable to trigger an internal validation failure.
    let properties = [minimq::Property::ResponseTopic(minimq::types::Utf8String(
        RESPONSE_TOPIC,
    ))];

    log::info!("Publishing error setting");
    mqtt.client()
        .publish(
            minimq::Publication::new(b"true")
                .topic("validation_failure/device/settings/error")
                .properties(&properties)
                .finish()
                .unwrap(),
        )
        .unwrap();

    // Wait until we get a response to the request.
    loop {
        if let Some(false) = mqtt
            .poll(|_client, _topic, message, _properties| {
                let data: Response = serde_json_core::from_slice(message).unwrap().0;
                assert!(data.code != 0);
                false
            })
            .unwrap()
        {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn main() {
    env_logger::init();

    // Spawn a task to send MQTT messages.
    tokio::task::spawn(async move { client_task().await });

    // Construct a settings configuration interface.
    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
    let mut interface: miniconf_mqtt::MqttClient<'_, _, _, _, minimq::broker::IpBroker, 1> =
        miniconf_mqtt::MqttClient::new(
            Stack,
            "validation_failure/device",
            StandardClock::default(),
            minimq::ConfigBuilder::new(localhost.into(), &mut buffer).keepalive_interval(60),
        )
        .unwrap();

    let mut settings = Settings::default();

    loop {
        interface.update(&mut settings).unwrap();

        if settings.error {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Check that the error setting did not stick.
    assert!(!settings.error);
}
