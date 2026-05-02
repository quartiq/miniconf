use clap::Parser;
use core::future::poll_fn;
use core::pin::Pin;
use core::task::Poll;
use embedded_io_async::{ErrorType, Read, Write};
use miniconf_mqtt::{Error, Event, Miniconf};
use minimq::{ConfigBuilder, ConnectEvent, Error as MqttError, MQTT_INSECURE_DEFAULT_PORT};
use std::net::SocketAddr;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

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

struct TokioConnection(TcpStream);

impl ErrorType for TokioConnection {
    type Error = std::io::Error;
}

impl Read for TokioConnection {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        poll_fn(|cx| {
            let mut read_buf = tokio::io::ReadBuf::new(buf);
            match Pin::new(&mut self.0).poll_read(cx, &mut read_buf) {
                Poll::Ready(Ok(())) => Poll::Ready(Ok(read_buf.filled().len())),
                Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
                Poll::Pending => Poll::Pending,
            }
        })
        .await
    }
}

impl Write for TokioConnection {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        poll_fn(|cx| match Pin::new(&mut self.0).poll_write(cx, buf) {
            Poll::Ready(Ok(0)) if !buf.is_empty() => {
                Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()))
            }
            Poll::Ready(result) => Poll::Ready(result),
            Poll::Pending => Poll::Pending,
        })
        .await
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        poll_fn(|cx| Pin::new(&mut self.0).poll_flush(cx)).await
    }
}

async fn connect_addr(addr: SocketAddr) -> std::io::Result<TokioConnection> {
    TcpStream::connect(addr).await.map(TokioConnection)
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let args = Args::parse();
    let broker = SocketAddr::new(
        args.broker.parse().expect("invalid broker address"),
        MQTT_INSECURE_DEFAULT_PORT,
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

fn config<'a>(buffer: &'a mut [u8], payload: usize, client_id: &str) -> ConfigBuilder<'a> {
    ConfigBuilder::from_buffer(buffer, payload)
        .unwrap()
        .client_id(client_id)
        .unwrap()
        .keepalive_interval(60)
}

async fn run(prefix: &str, broker: SocketAddr, client_id: &str) {
    let mut buffer = [0u8; 4096];
    let (mut mm2, mut session) = Miniconf::<common::Settings>::new::<TokioConnection>(
        prefix,
        config(&mut buffer, 1024, client_id),
    )
    .unwrap();
    let mut settings = common::Settings::new();
    println!("Serving common fixture on {prefix}");

    loop {
        let io = connect_addr(broker).await.unwrap();
        match session.connect(io).await.unwrap() {
            ConnectEvent::Connected => {
                mm2.activate(&mut session, &settings).await.unwrap();
                println!("Connected");
            }
            ConnectEvent::Reconnected => {
                mm2.publish_alive(&mut session).await.unwrap();
                println!("Reconnected");
            }
        }

        loop {
            match mm2.poll_with(&mut session, &mut settings, |_| ()).await {
                Ok(Event::Unhandled(())) => {}
                Ok(Event::Changed(_)) => {
                    println!("Settings updated");
                }
                Err(Error::Mqtt(MqttError::Disconnected)) => break,
                Err(err) => panic!("{err}"),
            }
        }
    }
}
