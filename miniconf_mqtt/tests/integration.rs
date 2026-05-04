use core::future::poll_fn;
use core::pin::Pin;
use core::task::Poll;
use embedded_io_async::{ErrorType, Read, Write};
use miniconf::Tree;
use miniconf_mqtt::{Event, Miniconf, Service, ServiceEvent};
use minimq::{
    ConfigBuilder, ConnectEvent, Publication, QoS, Session,
    types::{SubscriptionOptions, TopicFilter},
};
use std::sync::OnceLock;
use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
    time::{Duration, timeout},
};

#[path = "../../miniconf/examples/common.rs"]
mod common;

const BROKER_ADDR_ENV: &str = "BROKER";

fn init_host_logging() {
    static HOST_LOGGING: OnceLock<()> = OnceLock::new();

    HOST_LOGGING.get_or_init(|| {
        let _ = env_logger::builder().is_test(true).try_init();
        defmt2log::init_from_current_exe().expect("initialize defmt host logger");
    });
}

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
    Some(raw.parse().unwrap())
}

fn unique(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("miniconf-mqtt-{label}-{nanos}")
}

fn config(client_id: &str) -> ConfigBuilder<'static> {
    let buffer = Box::leak(Box::new([0; 2048]));
    ConfigBuilder::from_buffer(buffer, 1024)
        .unwrap()
        .client_id(client_id)
        .unwrap()
}

fn compact_config(client_id: &str) -> ConfigBuilder<'static> {
    let buffer = Box::leak(Box::new([0; 640]));
    ConfigBuilder::from_buffer(buffer, 128)
        .unwrap()
        .client_id(client_id)
        .unwrap()
}

#[derive(Debug)]
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

async fn connect_mm2<'a>(
    mm2: &mut Miniconf<Settings>,
    session: &mut Session<'a, TokioConnection>,
    settings: &Settings,
    io: TokioConnection,
) {
    let event = timeout(Duration::from_secs(5), session.connect(io))
        .await
        .unwrap()
        .unwrap();
    timeout(
        Duration::from_secs(5),
        mm2.startup(session, settings, event),
    )
    .await
    .unwrap()
    .unwrap();
}

async fn wait_session(session: &mut Session<'_, TokioConnection>, io: TokioConnection) {
    assert!(matches!(
        timeout(Duration::from_secs(5), session.connect(io))
            .await
            .unwrap()
            .unwrap(),
        ConnectEvent::Connected | ConnectEvent::Reconnected
    ));
}

#[tokio::test]
async fn mm2_set_stays_internal() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("prefix");
    let mut publisher = Session::new(config(&unique("pub")));
    let (mut mm2, mut session) =
        Miniconf::<Settings>::new::<TokioConnection>(&prefix, config(&unique("mm2"))).unwrap();
    let mut settings = Settings::default();

    connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    wait_session(&mut publisher, connect_addr(addr).await.unwrap()).await;
    publisher
        .publish(Publication::bytes(&format!("{prefix}/set/value"), b"9"))
        .await
        .unwrap();

    match timeout(
        Duration::from_secs(5),
        mm2.serve(&mut session, &mut settings, |_| ()),
    )
    .await
    .unwrap()
    .unwrap()
    {
        Event::Unhandled(_) => panic!("unexpected app traffic"),
        Event::Changed(_) => {}
    }
    assert_eq!(settings.value, 9);
}

#[tokio::test]
async fn other_topics_are_unhandled() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("prefix");
    let other_topic = format!("{prefix}/rpc/in");
    let mut publisher = Session::new(config(&unique("pub")));
    let (mut mm2, mut session) =
        Miniconf::<Settings>::new::<TokioConnection>(&prefix, config(&unique("sub"))).unwrap();
    let settings = Settings::default();

    wait_session(&mut publisher, connect_addr(addr).await.unwrap()).await;
    publisher
        .publish(
            Publication::bytes(&other_topic, b"hello")
                .retain()
                .qos(QoS::AtLeastOnce),
        )
        .await
        .unwrap();

    connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    let topics = [TopicFilter::new(&other_topic)
        .options(SubscriptionOptions::default().maximum_qos(QoS::AtMostOnce))];
    session.subscribe(&topics, &[]).await.unwrap();
    match timeout(
        Duration::from_secs(5),
        mm2.serve(&mut session, &mut Settings::default(), |message| {
            (message.topic().to_owned(), message.payload().to_vec())
        }),
    )
    .await
    .unwrap()
    .unwrap()
    {
        Event::Unhandled((topic, payload)) => {
            assert_eq!(topic, other_topic);
            assert_eq!(payload, b"hello");
        }
        Event::Changed(_) => panic!("unexpected MM2 handling"),
    }
}

#[tokio::test]
async fn startup_with_large_schema_waits_on_session_progress() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("activation");
    let (mut mm2, mut session) = Miniconf::<common::Settings>::new::<TokioConnection>(
        &prefix,
        compact_config(&unique("mm2")),
    )
    .unwrap();
    let settings = common::Settings::new();

    assert!(matches!(
        timeout(
            Duration::from_secs(5),
            session.connect(connect_addr(addr).await.unwrap())
        )
        .await
        .unwrap()
        .unwrap(),
        ConnectEvent::Connected
    ));

    let mut startup = mm2.begin_startup(ConnectEvent::Connected);
    let mut retries = 0usize;
    let mut saw_non_quiescent = false;
    let mut saw_internal_progress = false;
    timeout(Duration::from_secs(5), async {
        while !startup
            .step(&mut mm2, &mut session, &settings)
            .await
            .unwrap()
        {
            retries += 1;
            saw_non_quiescent |= !session.is_publish_quiescent();
            saw_internal_progress |= session.poll().await.unwrap().is_none();
        }
    })
    .await
    .unwrap();

    assert!(retries > 1, "startup never needed a retry");
    assert!(
        saw_non_quiescent,
        "startup never observed in-flight retained publishes"
    );
    assert!(
        saw_internal_progress,
        "startup never waited on internal-only session progress"
    );
    assert!(session.is_publish_quiescent());
}

#[tokio::test]
async fn service_accepts_later_sets_while_earlier_response_is_pending() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("queue");
    let mut publisher = Session::new(config(&unique("pub")));
    let (mut mm2, mut session) =
        Miniconf::<Settings>::new::<TokioConnection>(&prefix, config(&unique("mm2"))).unwrap();
    let mut settings = Settings::default();
    let mut service = Service::<4>::new();

    connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    wait_session(&mut publisher, connect_addr(addr).await.unwrap()).await;
    publisher
        .publish(Publication::bytes(&format!("{prefix}/set/value"), b"9"))
        .await
        .unwrap();
    publisher
        .publish(Publication::bytes(
            &format!("{prefix}/set/nested/leaf"),
            b"7",
        ))
        .await
        .unwrap();

    let mut accepted = 0usize;
    let mut saw_backlog = false;
    timeout(Duration::from_secs(5), async {
        while accepted < 2 {
            let Some(inbound) = session.poll().await.unwrap() else {
                continue;
            };
            match service.handle(&mut mm2, &mut settings, inbound) {
                ServiceEvent::Unhandled(_) => panic!("unexpected app traffic"),
                ServiceEvent::Idle | ServiceEvent::Busy => {}
                ServiceEvent::Changed(_) => {
                    accepted += 1;
                    saw_backlog |= service.len() > 1;
                }
            }
        }

        while !service.is_empty() {
            let _ = service
                .step(&mut mm2, &mut session, &mut settings)
                .await
                .unwrap();
            let _ = session.poll().await.unwrap();
        }
    })
    .await
    .unwrap();

    assert_eq!(accepted, 2);
    assert!(service.is_empty());
    assert!(saw_backlog);
    assert_eq!(settings.value, 9);
    assert_eq!(settings.nested.leaf, 7);
}

#[tokio::test]
async fn service_rejects_overflow_without_mutating() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("queue-overflow");
    let mut publisher = Session::new(config(&unique("pub")));
    let (mut mm2, mut session) =
        Miniconf::<Settings>::new::<TokioConnection>(&prefix, config(&unique("mm2"))).unwrap();
    let mut settings = Settings::default();
    let mut service = Service::<1>::new();

    connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    wait_session(&mut publisher, connect_addr(addr).await.unwrap()).await;
    publisher
        .publish(Publication::bytes(&format!("{prefix}/set/value"), b"9"))
        .await
        .unwrap();
    publisher
        .publish(Publication::bytes(
            &format!("{prefix}/set/nested/leaf"),
            b"7",
        ))
        .await
        .unwrap();

    let first = loop {
        let inbound = timeout(Duration::from_secs(5), session.poll())
            .await
            .unwrap()
            .unwrap();
        if let Some(inbound) = inbound {
            break inbound;
        }
    };
    assert!(matches!(
        service.handle(&mut mm2, &mut settings, first),
        ServiceEvent::Changed(_)
    ));

    let second = loop {
        let inbound = timeout(Duration::from_secs(5), session.poll())
            .await
            .unwrap()
            .unwrap();
        if let Some(inbound) = inbound {
            break inbound;
        }
    };
    assert!(matches!(
        service.handle(&mut mm2, &mut settings, second),
        ServiceEvent::Busy
    ));

    assert_eq!(settings.value, 9);
    assert_eq!(settings.nested.leaf, 0);
}
