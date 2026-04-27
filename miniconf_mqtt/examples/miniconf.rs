use clap::Parser;
use core::future::poll_fn;
use core::pin::Pin;
use core::task::Poll;
use embedded_io_async::{ErrorType, Read, ReadReady, Write, WriteReady};
use miniconf_mqtt::{Event, MqttClient, minimq};
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

impl ReadReady for TokioConnection {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        match self.0.try_io(tokio::io::Interest::READABLE, || Ok(())) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err),
        }
    }
}

impl WriteReady for TokioConnection {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        match self.0.try_io(tokio::io::Interest::WRITABLE, || Ok(())) {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err),
        }
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

fn config<'a>(buffer: &'a mut [u8], payload: usize, client_id: &str) -> minimq::ConfigBuilder<'a> {
    minimq::ConfigBuilder::from_buffer(buffer, payload)
        .unwrap()
        .client_id(client_id)
        .unwrap()
        .keepalive_interval(60)
}

async fn run(prefix: &str, broker: SocketAddr, client_id: &str) {
    let mut buffer = [0u8; 4096];
    let mut client = MqttClient::<_, _>::new(prefix, config(&mut buffer, 1024, client_id)).unwrap();

    let mut settings = common::Settings::new();
    println!("Serving common fixture on {prefix}");
    match client
        .connect(
            connect_addr(broker).await.expect("tcp connect failed"),
            &mut settings,
        )
        .await
    {
        Ok(Event::Connected) => println!("Connected"),
        Ok(Event::Reconnected) => println!("Reconnected"),
        Ok(other) => panic!("unexpected connect result: {other:?}"),
        Err(err) => {
            eprintln!("connect error: {err}");
            return;
        }
    }
    loop {
        match client.poll(&mut settings, |_| {}).await {
            Ok(Event::Changed) => println!("Settings updated"),
            Ok(_) => {}
            Err(err @ miniconf_mqtt::Error::Mqtt(minimq::Error::Disconnected)) => {
                eprintln!("poll error: {err}");
                let io = match connect_addr(broker).await {
                    Ok(io) => io,
                    Err(err) => {
                        eprintln!("tcp connect error: {err}");
                        continue;
                    }
                };
                match client.connect(io, &mut settings).await {
                    Ok(Event::Connected) => println!("Connected"),
                    Ok(Event::Reconnected) => println!("Reconnected"),
                    Ok(other) => panic!("unexpected connect result: {other:?}"),
                    Err(err) => eprintln!("connect error: {err}"),
                }
            }
            Err(err) => eprintln!("poll error: {err}"),
        }
    }
}
