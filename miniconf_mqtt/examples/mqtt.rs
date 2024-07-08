use miniconf::Tree;
use std::time::Duration;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Default, Tree, Debug)]
struct Inner {
    frame_rate: u32,
}

#[derive(Clone, Default, Tree, Debug)]
struct Settings {
    #[tree(depth = 1)]
    inner: Inner,
    #[tree(depth = 1)]
    amplitude: [f32; 2],
    array: [i32; 4],
    exit: bool,
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut client = miniconf_mqtt::MqttClient::new(
        Stack,
        "sample/prefix",
        StandardClock::default(),
        minimq::ConfigBuilder::<'_, minimq::broker::IpBroker>::new(localhost.into(), &mut buffer)
            .keepalive_interval(60),
    )
    .unwrap();

    let mut settings = Settings::default();
    while !settings.exit {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if client.update(&mut settings).unwrap() {
            println!("Settings updated: {:?}", settings);
        }
    }
}
