use heapless::String;
use miniconf::{Keys, Leaf, SerdeError, Tree, TreeDeserialize, ValueError};
use serde::{Deserialize, Deserializer, Serialize};
use std::time::Duration;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;

#[derive(Clone, Default, Tree, Debug)]
struct Inner {
    a: Leaf<u32>,
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
    stream: Leaf<String<32>>,
    afe: [Leaf<Gain>; 2],
    inner: Inner,
    values: [Leaf<f32>; 2],
    array: Leaf<[i32; 4]>,
    opt: Option<Leaf<i32>>,
    #[tree(with(deserialize=self.deserialize_four))]
    four: Leaf<f32>,
    exit: Leaf<bool>,
}

impl Settings {
    fn deserialize_four<'de, K: Keys, D: Deserializer<'de>>(
        &mut self,
        keys: K,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        let old = *self.four;
        self.four.deserialize_by_key(keys, de)?;
        if *self.four < 4.0 {
            *self.four = old;
            Err(ValueError::Access("Less than four").into())
        } else {
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut buffer = [0u8; 1024];
    let localhost: core::net::IpAddr = "127.0.0.1".parse().unwrap();

    // Construct a settings configuration interface.
    let mut client = miniconf_mqtt::MqttClient::<_, _, _, _, 4>::new(
        Stack,
        "test/id",
        StandardClock::default(),
        minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer)
            .keepalive_interval(60),
    )
    .unwrap();
    client.set_alive("\"hello\"");

    let mut settings = Settings::default();
    while !*settings.exit {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if client.update(&mut settings).unwrap() {
            println!("Settings updated: {:?}", settings);
        }
    }
    println!("Exiting on request");
}
