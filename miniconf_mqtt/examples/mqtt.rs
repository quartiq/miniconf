use heapless::String;
use miniconf::Tree;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Default, Tree, Debug)]
struct Inner {
    a: u32,
}

#[derive(Copy, Clone, Default, Debug, Serialize, Deserialize)]
enum Gain {
    #[default]
    G1,
    G10,
    G100,
}

#[derive(Clone, Default, Tree, Debug)]
struct Settings {
    stream: String<32>,
    #[tree(depth = 1)]
    afe: [Gain; 2],
    #[tree(depth = 1)]
    inner: Inner,
    #[tree(depth = 1)]
    values: [f32; 2],
    array: [i32; 4],
    #[tree(depth = 1)]
    opt: Option<i32>,
    #[tree(validate=Self::validate_four)]
    four: f32,
    exit: bool,
}

impl Settings {
    fn validate_four(&mut self, new: f32) -> Result<f32, &'static str> {
        if new < 4.0 {
            Err("Less than four")
        } else {
            Ok(new)
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut buffer = [0u8; 1024];
    let localhost: minimq::embedded_nal::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut client = miniconf_mqtt::MqttClient::new(
        Stack,
        "test/id",
        StandardClock::default(),
        minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer)
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
    println!("Exiting on request");
}
