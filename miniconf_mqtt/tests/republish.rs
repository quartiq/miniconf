use miniconf::Tree;
use minimq;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Debug, Default, Tree)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Clone, Debug, Default, Tree)]
struct Settings {
    data: u32,
    #[tree(depth = 1)]
    more: AdditionalSettings,
}

async fn verify_settings() {
    // Construct a Minimq client to the broker for publishing requests.
    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();
    let mut mqtt = minimq::Minimq::new(
        Stack,
        StandardClock::default(),
        minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer)
            .client_id("tester")
            .unwrap()
            .keepalive_interval(60),
    );

    // Wait for the broker connection
    while !mqtt.client().is_connected() {
        mqtt.poll(|_client, _topic, _message, _properties| {})
            .unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Subscribe to the settings topic.
    mqtt.client()
        .subscribe(
            &[minimq::types::TopicFilter::new(
                "republish/device/settings/#",
            )],
            &[],
        )
        .unwrap();

    // Make sure the device republished all available settings.
    let mut received_settings = std::collections::HashMap::from([
        ("republish/device/settings/data", 0),
        ("republish/device/settings/more/inner", 0),
    ]);

    for _ in 0..300 {
        // 3 seconds
        mqtt.poll(|_, topic, value, _properties| {
            log::info!("{}: {:?}", topic, value);
            let element = received_settings.get_mut(topic).unwrap();
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

    let task = tokio::task::spawn(verify_settings());

    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    let mut interface = miniconf_mqtt::MqttClient::new(
        Stack,
        "republish/device",
        StandardClock::default(),
        minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer)
            .keepalive_interval(60),
    )
    .unwrap();

    let mut settings = Settings::default();

    for _ in 0..300 {
        // 3 s > REPUBLISH_TIMEOUT_SECONDS
        // The interface should never indicate a settings update during the republish process.
        assert!(!interface.update(&mut settings).unwrap());
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Next, verify that all of the settings have been published by the client.
    task.await.expect("Not all settings received");
}
