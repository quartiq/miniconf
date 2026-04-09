use heapless::String;
use miniconf::{Leaf, Tree, TreeSchema, leaf};
use miniconf_mqtt::minimq::{
    self, Broker, BufferLayout,
    embedded_io_async::{ErrorKind, ErrorType, Read, Write},
    timer::Timer,
    transport::Connector,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{SocketAddr, TcpStream},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

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

#[derive(Debug)]
struct StdConnection(TcpStream);

impl ErrorType for StdConnection {
    type Error = ErrorKind;
}

fn io_kind(kind: io::ErrorKind) -> ErrorKind {
    match kind {
        io::ErrorKind::NotFound => ErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => ErrorKind::PermissionDenied,
        io::ErrorKind::ConnectionRefused => ErrorKind::ConnectionRefused,
        io::ErrorKind::ConnectionReset => ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted => ErrorKind::ConnectionAborted,
        io::ErrorKind::NotConnected => ErrorKind::NotConnected,
        io::ErrorKind::AddrInUse => ErrorKind::AddrInUse,
        io::ErrorKind::AddrNotAvailable => ErrorKind::AddrNotAvailable,
        io::ErrorKind::BrokenPipe => ErrorKind::BrokenPipe,
        io::ErrorKind::AlreadyExists => ErrorKind::AlreadyExists,
        io::ErrorKind::InvalidInput => ErrorKind::InvalidInput,
        io::ErrorKind::InvalidData => ErrorKind::InvalidData,
        io::ErrorKind::TimedOut => ErrorKind::TimedOut,
        io::ErrorKind::WouldBlock => ErrorKind::TimedOut,
        io::ErrorKind::Interrupted => ErrorKind::Interrupted,
        io::ErrorKind::Unsupported => ErrorKind::Unsupported,
        io::ErrorKind::OutOfMemory => ErrorKind::OutOfMemory,
        io::ErrorKind::WriteZero => ErrorKind::WriteZero,
        _ => ErrorKind::Other,
    }
}

impl Read for StdConnection {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        io::Read::read(&mut self.0, buf).map_err(|err| io_kind(err.kind()))
    }
}

impl Write for StdConnection {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        io::Write::write(&mut self.0, buf).map_err(|err| io_kind(err.kind()))
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        io::Write::flush(&mut self.0).map_err(|err| io_kind(err.kind()))
    }
}

#[derive(Debug, Copy, Clone)]
struct StdConnector;

impl Connector for StdConnector {
    type Connection<'a> = StdConnection;

    async fn connect<'a>(&'a self, broker: &Broker) -> Result<Self::Connection<'a>, minimq::Error> {
        let remote = match broker {
            Broker::SocketAddr(addr) => *addr,
            Broker::Hostname { .. } => {
                return Err(minimq::Error::Transport(ErrorKind::Unsupported));
            }
        };
        let stream = TcpStream::connect_timeout(&remote, Duration::from_secs(5))
            .map_err(|err| minimq::Error::Transport(io_kind(err.kind())))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|err| minimq::Error::Transport(io_kind(err.kind())))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|err| minimq::Error::Transport(io_kind(err.kind())))?;
        stream
            .set_nodelay(true)
            .map_err(|err| minimq::Error::Transport(io_kind(err.kind())))?;
        Ok(StdConnection(stream))
    }
}

#[derive(Default)]
struct TokioTimer;

impl Timer for TokioTimer {
    type Error = core::convert::Infallible;

    fn now(&mut self) -> Result<u64, Self::Error> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64)
    }

    async fn sleep_until(&mut self, deadline_ms: u64) -> Result<(), Self::Error> {
        let now = self.now().unwrap();
        if deadline_ms > now {
            tokio::time::sleep(Duration::from_millis(deadline_ms - now)).await;
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let mut buffer = [0u8; 2048];
    let broker = Broker::socket_addr(SocketAddr::new(
        "127.0.0.1".parse().unwrap(),
        minimq::MQTT_INSECURE_DEFAULT_PORT,
    ));
    let connector = StdConnector;

    const MAX_DEPTH: usize = Settings::SCHEMA.shape().max_depth;

    let mut client = miniconf_mqtt::MqttClient::<_, _, _, MAX_DEPTH>::new(
        "test/id",
        &connector,
        TokioTimer,
        minimq::ConfigBuilder::from_buffer_layout(
            broker,
            &mut buffer,
            BufferLayout {
                rx: 512,
                tx: 512,
                inflight: 1024,
            },
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
            Ok(miniconf_mqtt::State::Unchanged) => {}
            Err(err) => panic!("{err:?}"),
        }
    }
    println!("Exiting on request");
}
