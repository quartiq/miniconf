use clap::Parser;
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
use embedded_nal_async::TcpConnect;
use miniconf_mqtt::{
    Event, MqttClient,
    minimq::{self, transport::TcpConnector},
};
use std::net::SocketAddr;
use std_embedded_nal_async::Stack as StdStack;

#[path = "../../miniconf/examples/common.rs"]
mod common;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "127.0.0.1")]
    broker: std::string::String,
    #[arg(long)]
    prefix: Option<std::string::String>,
    #[arg(long)]
    client_id: Option<std::string::String>,
}

fn io_kind(err: &std::io::Error) -> ErrorKind {
    use std::io::ErrorKind as K;
    match err.kind() {
        K::NotFound => ErrorKind::NotFound,
        K::PermissionDenied => ErrorKind::PermissionDenied,
        K::ConnectionRefused => ErrorKind::ConnectionRefused,
        K::ConnectionReset => ErrorKind::ConnectionReset,
        K::ConnectionAborted => ErrorKind::ConnectionAborted,
        K::NotConnected => ErrorKind::NotConnected,
        K::AddrInUse => ErrorKind::AddrInUse,
        K::AddrNotAvailable => ErrorKind::AddrNotAvailable,
        K::BrokenPipe => ErrorKind::BrokenPipe,
        K::AlreadyExists => ErrorKind::AlreadyExists,
        K::InvalidInput => ErrorKind::InvalidInput,
        K::TimedOut => ErrorKind::TimedOut,
        K::Interrupted => ErrorKind::Interrupted,
        K::Unsupported => ErrorKind::Unsupported,
        K::UnexpectedEof => ErrorKind::Other,
        K::OutOfMemory => ErrorKind::OutOfMemory,
        K::WriteZero => ErrorKind::WriteZero,
        _ => ErrorKind::Other,
    }
}

#[derive(Default)]
struct Stack(StdStack);

struct Socket<'a>(<StdStack as TcpConnect>::Connection<'a>);

impl ErrorType for Socket<'_> {
    type Error = ErrorKind;
}

impl Read for Socket<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await.map_err(|err| io_kind(&err))
    }
}

impl Write for Socket<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.0.write(buf).await.map_err(|err| io_kind(&err))
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.flush().await.map_err(|err| io_kind(&err))
    }
}

impl TcpConnect for Stack {
    type Error = ErrorKind;
    type Connection<'a> = Socket<'a>;

    async fn connect<'a>(
        &'a self,
        remote: SocketAddr,
    ) -> Result<Self::Connection<'a>, Self::Error> {
        self.0
            .connect(remote)
            .await
            .map(Socket)
            .map_err(|err| io_kind(&err))
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let args = Args::parse();

    let broker = SocketAddr::new(
        args.broker.parse().expect("invalid broker address"),
        minimq::MQTT_INSECURE_DEFAULT_PORT,
    );

    run(
        args.prefix.as_deref().unwrap_or("test/common"),
        broker,
        args.client_id
            .as_deref()
            .unwrap_or("miniconf-common-fixture"),
    )
    .await;
}

fn config<'a>(
    broker: SocketAddr,
    buffer: &'a mut [u8],
    payload: usize,
    client_id: &str,
) -> minimq::ConfigBuilder<'a> {
    minimq::ConfigBuilder::from_buffer(broker.into(), buffer, payload)
        .unwrap()
        .client_id(client_id)
        .unwrap()
        .keepalive_interval(60)
}

async fn run(prefix: &str, broker: SocketAddr, client_id: &str) {
    let mut buffer = [0u8; 4096];
    let connector = TcpConnector::new(Stack::default());

    let mut client = MqttClient::<_, _>::new(
        prefix,
        &connector,
        config(broker, &mut buffer, 1024, client_id),
    )
    .unwrap();

    let mut settings = common::Settings::new();
    println!("Serving common fixture on {prefix}");
    loop {
        match client.poll(&mut settings, |_| {}).await {
            Ok(Event::Changed) => println!("Settings updated"),
            Ok(_) => {}
            Err(err) => eprintln!("poll error: {err}"),
        }
    }
}
