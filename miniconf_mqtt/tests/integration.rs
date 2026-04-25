use core::future::poll_fn;
use core::pin::Pin;
use core::task::Poll;
use embedded_io_async::{ErrorType, Read, ReadReady, Write, WriteReady};
use miniconf::Tree;
use miniconf_mqtt::{Event, MqttClient, minimq};
use minimq::{
    Broker, ConnectEvent, Publication, QoS, Session,
    transport::Connector,
    types::{SubscriptionOptions, TopicFilter},
};
use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};

const BROKER_ADDR_ENV: &str = "MINICONF_MQTT_REAL_BROKER_ADDR";

#[derive(Tree, Default)]
struct Nested {
    leaf: u8,
}

#[derive(Tree, Default)]
struct Settings {
    value: u8,
    nested: Nested,
}

fn broker_addr() -> Option<SocketAddr> {
    let raw = std::env::var(BROKER_ADDR_ENV).ok()?;
    Some(
        raw.parse()
            .unwrap_or_else(|_| panic!("invalid {BROKER_ADDR_ENV} value: {raw}")),
    )
}

fn unique(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("miniconf-mqtt-{label}-{nanos}")
}

fn config<'a>(broker: Broker<'a>, client_id: &str) -> minimq::ConfigBuilder<'a> {
    let buffer = Box::leak(Box::new([0; 2048]));
    minimq::ConfigBuilder::from_buffer(broker, buffer, 1024)
        .unwrap()
        .client_id(client_id)
        .unwrap()
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

#[derive(Copy, Clone)]
struct TokioConnector;

impl Connector for TokioConnector {
    type Error = std::io::Error;
    type Connection<'a> = TokioConnection;

    async fn connect<'a>(
        &'a self,
        broker: &Broker<'_>,
    ) -> Result<Self::Connection<'a>, Self::Error> {
        let Broker::SocketAddr(addr) = broker else {
            return Err(std::io::ErrorKind::Unsupported.into());
        };
        TcpStream::connect(*addr).await.map(TokioConnection)
    }
}

async fn wait_client(
    client: &mut MqttClient<'_, Settings, TokioConnector>,
    settings: &mut Settings,
    mut on_other: impl FnMut(&minimq::InboundPublish<'_>),
    want: impl Fn(Event) -> bool,
) -> Event {
    for _ in 0..200 {
        let event = client
            .poll(settings, |message| on_other(message))
            .await
            .unwrap();
        if want(event) {
            return event;
        }
    }
    panic!("timed out waiting for client event");
}

async fn connect_client(
    client: &mut MqttClient<'_, Settings, TokioConnector>,
    settings: &mut Settings,
) -> Event {
    client.connect(settings).await.unwrap()
}

async fn wait_session(session: &mut Session<'_, '_, TokioConnector>) {
    assert!(matches!(
        session.connect().await.unwrap(),
        ConnectEvent::Connected | ConnectEvent::Reconnected
    ));
}

#[tokio::test]
async fn mm2_set_stays_internal() {
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let connector = TokioConnector;
    let prefix = unique("prefix");
    let mut publisher = Session::new(config(addr.into(), &unique("pub")), &connector);
    let mut client =
        MqttClient::<Settings, _>::new(&prefix, &connector, config(addr.into(), &unique("mm2")))
            .unwrap();
    let mut settings = Settings::default();

    assert!(matches!(
        connect_client(&mut client, &mut settings).await,
        Event::Connected | Event::Reconnected
    ));

    wait_session(&mut publisher).await;
    publisher
        .publish(Publication::bytes(&format!("{prefix}/set/value"), b"9"))
        .await
        .unwrap();

    let mut callback_called = false;
    let event = wait_client(
        &mut client,
        &mut settings,
        |_| callback_called = true,
        |event| matches!(event, Event::Changed),
    )
    .await;
    assert_eq!(event, Event::Changed);
    assert_eq!(settings.value, 9);
    assert!(!callback_called);
}

#[tokio::test]
async fn other_topics_reach_callback() {
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let connector = TokioConnector;
    let prefix = unique("prefix");
    let other_topic = format!("{prefix}/rpc/in");
    let mut publisher = Session::new(config(addr.into(), &unique("pub")), &connector);
    let mut client =
        MqttClient::<Settings, _>::new(&prefix, &connector, config(addr.into(), &unique("sub")))
            .unwrap();
    let mut settings = Settings::default();

    assert!(matches!(
        connect_client(&mut client, &mut settings).await,
        Event::Connected
    ));
    let topics = [TopicFilter::new(&other_topic)
        .options(SubscriptionOptions::default().maximum_qos(QoS::AtMostOnce))];
    client.subscribe(&topics, &[]).await.unwrap();
    let _ = wait_client(&mut client, &mut settings, |_| {}, |_| true).await;

    wait_session(&mut publisher).await;
    publisher
        .publish(Publication::bytes(&other_topic, b"hello"))
        .await
        .unwrap();

    let mut seen = None;
    let event = wait_client(
        &mut client,
        &mut settings,
        |message| seen = Some((message.topic().to_owned(), message.payload().to_vec())),
        |event| matches!(event, Event::Other),
    )
    .await;
    assert_eq!(event, Event::Other);
    assert_eq!(seen, Some((other_topic, b"hello".to_vec())));
}
