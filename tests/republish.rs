use tokio;

use miniconf::{minimq, Miniconf};
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Debug, Default, Miniconf)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Debug, Default, Miniconf)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

async fn verify_settings() {
    // Construct a Minimq client to the broker for publishing requests.
    let mut mqtt: minimq::Minimq<_, _, 256, 1> = miniconf::minimq::Minimq::new(
        "127.0.0.1".parse().unwrap(),
        "tester",
        Stack::default(),
        StandardClock::default(),
    )
    .unwrap();

    // Wait for the broker connection
    while !mqtt.client.is_connected() {
        mqtt.poll(|_client, _topic, _message, _properties| {})
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Subscribe to the settings topic.
    mqtt.client
        .subscribe("republish/device/settings/#", &[])
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

        if received_settings
            .iter()
            .map(|(_, value)| value)
            .all(|&x| x >= 1)
        {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Ensure that all fields were iterated exactly once.
    assert!(received_settings
        .iter()
        .map(|(_, value)| value)
        .all(|&x| x == 1));
}

#[tokio::test]
async fn main() {
    env_logger::init();

    // Spawn a task to send MQTT messages.
    let task = tokio::task::spawn(async move { verify_settings().await });

    // Construct a settings configuration interface.
    let mut interface: miniconf::MqttClient<Settings, _, _, 256> = miniconf::MqttClient::new(
        Stack::default(),
        "",
        "republish/device",
        "127.0.0.1".parse().unwrap(),
        StandardClock::default(),
    )
    .unwrap();

    // Poll the client for 5 seconds. This should be enough time for the miniconf client to publish
    // all settings values.
    for _ in 0..500 {
        interface.update().unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Next, verify that all of the settings have been published by the client.
    task.await.expect("Not all settings received");
}
