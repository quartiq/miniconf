use heapless::String;
use miniconf::{Leaf, Tree, TreeSchema, leaf};
use miniconf_mqtt::minimq::{BufferLayout, transport::TcpConnector};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, time::Duration};
use std_embedded_nal_async::Stack;

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

#[allow(dead_code)]
#[derive(Clone, Tree, Debug)]
enum Mode {
    A(u8),
    B(Inner),
}

impl Default for Mode {
    fn default() -> Self {
        Self::A(0)
    }
}

#[derive(Clone, Default, Tree, Debug)]
struct Control {
    #[tree(attrs(switches = "mode"))]
    tag: String<8>,
    mode: Mode,
}

#[derive(Clone, Default, Tree, Debug)]
struct Settings {
    stream: String<32>,
    afe: [Leaf<Gain>; 2],
    control: Control,
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

    let mut buffer = [0u8; 2048];
    let broker = SocketAddr::new(
        "127.0.0.1".parse().unwrap(),
        minimq::MQTT_INSECURE_DEFAULT_PORT,
    )
    .into();
    let connector = TcpConnector::new(Stack::default());

    const MAX_DEPTH: usize = Settings::SCHEMA.shape().max_depth + 1;

    let mut client = miniconf_mqtt::MqttClient::<_, _, MAX_DEPTH>::new(
        "test/id",
        &connector,
        minimq::ConfigBuilder::from_buffer_layout(
            broker,
            &mut buffer,
            BufferLayout { rx: 512, tx: 1536 },
        )
        .unwrap()
        .client_id("miniconf-mqtt-example")
        .unwrap()
        .keepalive_interval(60),
    )
    .unwrap();
    client.set_alive("\"hello\"");
    client.dump(None).unwrap();

    let mut settings = Settings::default();
    while !settings.exit {
        tokio::time::sleep(Duration::from_millis(10)).await;
        match client.poll(&mut settings).await {
            Ok(miniconf_mqtt::State::Changed) => println!("Settings updated: {:?}", settings),
            Ok(_) => {}
            Err(err) => panic!("{err}"),
        }
    }
    println!("Exiting on request");
}
