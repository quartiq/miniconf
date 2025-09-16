use heapless::String;
use miniconf::{Leaf, Tree, TreeSchema, leaf};
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
    afe: [Leaf<Gain>; 2],
    inner: Inner,
    values: [f32; 2],
    #[tree(with=leaf)]
    array: [i32; 4],
    opt: Option<i32>,
    #[tree(with=four)]
    four: f32,
    exit: bool,
}

mod four {
    use miniconf::{Deserializer, Keys, SerdeError, TreeDeserialize, ValueError, leaf};

    pub use leaf::{SCHEMA, mut_any_by_key, probe_by_key, ref_any_by_key, serialize_by_key};

    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        value: &mut f32,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        let mut old = *value;
        old.deserialize_by_key(keys, de)?;
        if old < 4.0 {
            Err(ValueError::Access("Less than four").into())
        } else {
            *value = old;
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut buffer = [0u8; 1024];
    let localhost: core::net::IpAddr = "127.0.0.1".parse().unwrap();

    const MAX_DEPTH: usize = Settings::SCHEMA.shape().max_depth;

    // Construct a settings configuration interface.
    let mut client = miniconf_mqtt::MqttClient::<_, _, _, _, MAX_DEPTH>::new(
        Stack,
        "test/id",
        StandardClock::default(),
        minimq::ConfigBuilder::<minimq::broker::IpBroker>::new(localhost.into(), &mut buffer)
            .keepalive_interval(60),
    )
    .unwrap();
    client.set_alive("\"hello\"");

    let mut settings = Settings::default();
    while !settings.exit {
        tokio::time::sleep(Duration::from_millis(10)).await;
        if client.update(&mut settings).unwrap() {
            println!("Settings updated: {:?}", settings);
        }
    }
    println!("Exiting on request");
}
