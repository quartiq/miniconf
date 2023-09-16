use miniconf::Tree;
use minimq::Publication;
use std::time::Duration;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Default, Tree, Debug)]
struct NestedSettings {
    frame_rate: u32,
}

#[derive(Clone, Default, Tree, Debug)]
struct Settings {
    #[tree()]
    inner: NestedSettings,

    #[tree()]
    amplitude: [f32; 2],

    exit: bool,
}

async fn mqtt_client() {
    // Construct a Minimq client to the broker for publishing requests.
    let mut buffer = [0u8; 1024];
    let mut mqtt: minimq::Minimq<'_, _, _, minimq::broker::NamedBroker<Stack>> =
        minimq::Minimq::new(
            Stack,
            StandardClock::default(),
            minimq::ConfigBuilder::new(
                minimq::broker::NamedBroker::new("localhost", Stack).unwrap(),
                &mut buffer,
            )
            .client_id("tester")
            .unwrap()
            .keepalive_interval(60),
        );

    // Wait for the broker connection
    while !mqtt.client().is_connected() {
        mqtt.poll(|_client, _topic, _message, _properties| {})
            .unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    log::info!("Test client connected");

    // Wait momentarily for the other client to connect.
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Configure settings.
    mqtt.client()
        .publish(
            Publication::new(b"32.4")
                .topic("sample/prefix/settings/amplitude/0")
                .finish()
                .unwrap(),
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    mqtt.client()
        .publish(
            Publication::new(b"10")
                .topic("sample/prefix/settings/inner/frame_rate")
                .finish()
                .unwrap(),
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    mqtt.client()
        .publish(
            Publication::new(b"true")
                .topic("sample/prefix/settings/exit")
                .finish()
                .unwrap(),
        )
        .unwrap();
}

#[tokio::main]
async fn main() {
    env_logger::init();

    // Spawn a task to send MQTT messages.
    tokio::task::spawn(async move { mqtt_client().await });

    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut client: miniconf::MqttClient<'_, _, _, _, minimq::broker::IpBroker, 2> =
        miniconf::MqttClient::new(
            Stack,
            "sample/prefix",
            StandardClock::default(),
            Settings::default(),
            minimq::ConfigBuilder::new(localhost.into(), &mut buffer).keepalive_interval(60),
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
