use miniconf::{Miniconf, MqttClient};
use minimq::{Minimq, QoS, Retain};
use std::time::Duration;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Default, Miniconf, Debug)]
struct NestedSettings {
    frame_rate: u32,
}

#[derive(Default, Miniconf, Debug)]
struct Settings {
    inner: NestedSettings,
    amplitude: [f32; 2],
    exit: bool,
}

async fn mqtt_client() {
    // Construct a Minimq client to the broker for publishing requests.
    let mut mqtt: Minimq<_, _, 256, 1> = Minimq::new(
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
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Wait momentarily for the other client to connect.
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Configure settings.
    mqtt.client
        .publish(
            "sample/prefix/settings/amplitude/0",
            b"32.4",
            QoS::AtMostOnce,
            Retain::NotRetained,
            &[],
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    mqtt.client
        .publish(
            "sample/prefix/settings/inner/frame_rate",
            b"10",
            QoS::AtMostOnce,
            Retain::NotRetained,
            &[],
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::main]
async fn main() {
    env_logger::init();
    // Spawn a task to send MQTT messages.
    tokio::task::spawn(async move { mqtt_client().await });

    let mut client: MqttClient<Settings, Stack, StandardClock, 256, 1> = MqttClient::new(
        Stack::default(),
        "",
        "sample/prefix",
        "127.0.0.1".parse().unwrap(),
        StandardClock::default(),
    )
    .unwrap();

    loop {
        if client.update().unwrap() {
            println!("Settings updated: {:?}", client.settings());
        }

        if client.settings().exit {
            break;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
