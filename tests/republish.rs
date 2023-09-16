#![cfg(feature = "mqtt-client")]

use miniconf::Tree;
use minimq::{self, types::TopicFilter};
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Debug, Default, Tree)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Clone, Debug, Default, Tree)]
struct Settings {
    data: u32,
    #[tree()]
    more: AdditionalSettings,
}

async fn verify_settings() {
    // Construct a Minimq client to the broker for publishing requests.
    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
    let mut mqtt: minimq::Minimq<'_, _, _, minimq::broker::IpBroker> = minimq::Minimq::new(
        Stack,
        StandardClock::default(),
        minimq::ConfigBuilder::new(localhost.into(), &mut buffer)
            .client_id("tester")
            .unwrap()
            .keepalive_interval(60),
    );

    // Wait for the broker connection
    while !mqtt.client().is_connected() {
        mqtt.poll(|_client, _topic, _message, _properties| {})
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Subscribe to the settings topic.
    mqtt.client()
        .subscribe(&[TopicFilter::new("republish/device/settings/#")], &[])
        .unwrap();

    // Wait the other device to connect and publish settings.
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Make sure the device republished all available settings.
    let mut received_settings = std::collections::HashMap::from([
        ("republish/device/settings/data".to_string(), 0),
        ("republish/device/settings/more/inner".to_string(), 0),
    ]);

    for _ in 0..50 {
        mqtt.poll(|_, topic, value, _properties| {
            log::info!("{}: {:?}", &topic, value);
            let element = received_settings.get_mut(&topic.to_string()).unwrap();
            *element += 1;
        })
        .unwrap();

        if received_settings.values().all(|&x| x >= 1) {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Ensure that all fields were iterated exactly once.
    assert!(received_settings.values().all(|&x| x == 1));
}

#[tokio::test]
async fn main() {
    env_logger::init();

    // Spawn a task to send MQTT messages.
    let task = tokio::task::spawn(async move { verify_settings().await });

    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut interface: miniconf::MqttClient<'_, _, _, _, minimq::broker::IpBroker, 2> =
        miniconf::MqttClient::new(
            Stack,
            "republish/device",
            StandardClock::default(),
            Settings::default(),
            minimq::ConfigBuilder::new(localhost.into(), &mut buffer).keepalive_interval(60),
        )
        .unwrap();

    // Poll the client for 5 seconds. This should be enough time for the miniconf client to publish
    // all settings values.
    for _ in 0..500 {
        // The interface should never indicate a settings update during the republish process.
        assert!(!interface.update().unwrap());
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Next, verify that all of the settings have been published by the client.
    task.await.expect("Not all settings received");
}
