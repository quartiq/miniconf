use miniconf::{minimq, Miniconf};
use serde::Deserialize;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Debug, Default, Miniconf)]
struct Settings {
    error: bool,
}

#[derive(Deserialize)]
struct Response {
    code: u8,
    _message: heapless::String<256>,
}

async fn client_task() {
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

    let response_topic = "validation_failure/device/response";
    mqtt.client.subscribe(&response_topic, &[]).unwrap();

    // Wait the other device to connect.
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

    // Configure the error variable to trigger an internal validation failure.
    let properties = [minimq::Property::ResponseTopic(&response_topic)];

    log::info!("Publishing error setting");
    mqtt.client
        .publish(
            "validation_failure/device/settings/error",
            b"true",
            minimq::QoS::AtMostOnce,
            minimq::Retain::NotRetained,
            &properties,
        )
        .unwrap();

    // Wait until we get a response to the request.
    let mut continue_testing = true;
    loop {
        mqtt.poll(|_client, _topic, message, _properties| {
            let data: Response = serde_json_core::from_slice(message).unwrap().0;
            assert!(data.code != 0);
            continue_testing = false;
        })
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        if !continue_testing {
            break;
        }
    }
}

#[tokio::test]
async fn main() {
    env_logger::init();

    // Spawn a task to send MQTT messages.
    tokio::task::spawn(async move { client_task().await });

    // Construct a settings configuration interface.
    let mut interface: miniconf::MqttClient<Settings, _, _, 256> = miniconf::MqttClient::new(
        Stack::default(),
        "",
        "validation_failure/device",
        "127.0.0.1".parse().unwrap(),
        StandardClock::default(),
    )
    .unwrap();

    // Update the client until the exit
    let mut should_exit = false;
    loop {
        interface
            .handled_update(|settings| {
                log::info!("Handling setting update");
                if settings.error {
                    should_exit = true;
                    return Err(());
                }

                return Ok(());
            })
            .unwrap();

        if should_exit {
            break;
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Check that the error setting did not stick.
    assert!(!interface.settings().error);
}
