use miniconf::{Miniconf, MqttClient};
use minimq::{Minimq, QoS};
use std::time::Duration;
use std_embedded_nal::Stack;

use embedded_time::{fraction::Fraction, Clock};
use std::time::Instant;

#[derive(Default)]
struct StdClock {
    start: core::cell::UnsafeCell<Option<Instant>>,
}

impl Clock for StdClock {
    type T = u32;

    const SCALING_FACTOR: Fraction = Fraction::new(1, 1_000);

    fn try_now(&self) -> Result<embedded_time::Instant<Self>, embedded_time::clock::Error> {
        let std_now = Instant::now();
        let start = unsafe {
            if (*self.start.get()).is_none() {
                (*self.start.get()).replace(std_now);
                std_now
            } else {
                (*self.start.get()).unwrap()
            }
        };

        let elapsed = std_now - start;

        Ok(embedded_time::Instant::new(elapsed.as_millis() as u32))
    }
}

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
    let mut mqtt: Minimq<_, 256> =
        Minimq::new("127.0.0.1".parse().unwrap(), "tester", Stack::default()).unwrap();

    // Wait for the broker connection
    while !mqtt.client.is_connected().unwrap() {
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
            &[],
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    mqtt.client
        .publish(
            "sample/prefix/settings/inner/frame_rate",
            b"10",
            QoS::AtMostOnce,
            &[],
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    mqtt.client
        .publish("sample/prefix/settings/exit", b"true", QoS::AtMostOnce, &[])
        .unwrap();
}

#[tokio::main]
async fn main() {
    // Spawn a task to send MQTT messages.
    tokio::task::spawn(async move { mqtt_client().await });

    let mut client: MqttClient<Settings, Stack> = MqttClient::new(
        Stack::default(),
        "",
        "sample/prefix",
        "127.0.0.1".parse().unwrap(),
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
