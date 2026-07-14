use embedded_io_adapters::tokio_1::FromTokio;
use miniconf::Tree;
use miniconf_mqtt::{Event, LoadRetained, Miniconf, Service, ServiceEvent};
use minimq::{
    ConfigBuilder, ConnectEvent, Connection, InboundPublish, Op, Property, Publication, QoS,
    RetainHandling, Session, SubscriptionOptions, TopicFilter,
};
use std::sync::OnceLock;
use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpStream,
    time::{Duration, timeout},
};

#[path = "../../miniconf/examples/common.rs"]
mod common;

const BROKER_ADDR_ENV: &str = "BROKER";

fn init_host_logging() {
    static HOST_LOGGING: OnceLock<()> = OnceLock::new();

    HOST_LOGGING.get_or_init(|| {
        env_logger::builder().is_test(true).try_init().unwrap();
        defmt2log::init_from_current_exe();
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
    Some(std::env::var(BROKER_ADDR_ENV).ok()?.parse().unwrap())
}

fn unique(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("miniconf-mqtt-{label}-{nanos}")
}

fn config() -> ConfigBuilder<'static> {
    ConfigBuilder::from_buffer(Box::leak(Box::new([0; 2048])), 1024).unwrap()
}

fn compact_config() -> ConfigBuilder<'static> {
    ConfigBuilder::from_buffer(Box::leak(Box::new([0; 640])), 128).unwrap()
}

type TokioConnection = FromTokio<TcpStream>;

async fn connect_addr(addr: SocketAddr) -> std::io::Result<TokioConnection> {
    Ok(FromTokio::new(TcpStream::connect(addr).await?))
}

async fn connect_mm2<'session, 'buf>(
    mm2: &mut Miniconf<Settings>,
    session: &'session mut Session<'buf>,
    settings: &Settings,
    io: TokioConnection,
) -> Connection<'session, 'buf, TokioConnection> {
    let mut connection = timeout(Duration::from_secs(5), session.connect(io))
        .await
        .unwrap()
        .unwrap();
    timeout(
        Duration::from_secs(5),
        mm2.startup(&mut connection, settings),
    )
    .await
    .unwrap()
    .unwrap();
    connection
}

async fn wait_session<'session, 'buf>(
    session: &'session mut Session<'buf>,
    io: TokioConnection,
) -> Connection<'session, 'buf, TokioConnection> {
    let connection = timeout(Duration::from_secs(5), session.connect(io))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        connection.connect_event(),
        ConnectEvent::Connected | ConnectEvent::Reconnected
    ));
    connection
}

async fn wait_op(connection: &mut Connection<'_, '_, TokioConnection>, op: Op) {
    timeout(Duration::from_secs(5), async {
        loop {
            if connection.is_complete(&op) {
                break;
            }
            if connection.is_invalidated(&op) {
                panic!("operation invalidated");
            }
            debug_assert!(connection.is_pending(&op));
            let _ = connection.poll().await.unwrap();
        }
    })
    .await
    .unwrap();
}

fn has_utf8_payload_indicator(inbound: &InboundPublish<'_>) -> bool {
    inbound
        .properties()
        .iter()
        .any(|prop| matches!(prop, Ok(Property::PayloadFormatIndicator(1))))
}

fn user_property<'a>(inbound: &'a InboundPublish<'a>, name: &str) -> Option<&'a str> {
    inbound.properties().iter().find_map(|prop| match prop {
        Ok(Property::UserProperty(key, value)) if key == name => Some(value),
        _ => None,
    })
}

#[tokio::test]
async fn mm2_publications_advertise_utf8_payloads() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("utf8-payload");
    let mut observer_session = Session::new(config());
    let mut observer = wait_session(&mut observer_session, connect_addr(addr).await.unwrap()).await;
    let prefix_filter = format!("{prefix}/#");
    let topics = [TopicFilter::new(&prefix_filter)
        .options(SubscriptionOptions::default().maximum_qos(QoS::AtLeastOnce))];
    observer.subscribe(&topics, &[]).await.unwrap();

    let (mut mm2, mut session) = Miniconf::<Settings>::new(&prefix, config()).unwrap();
    let mut settings = Settings::default();
    let mut connection = connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    let mut saw_alive = false;
    let mut saw_schema = false;
    let mut saw_value = false;
    let mut saw_nested = false;
    let alive_topic = format!("{prefix}/alive");
    let schema_prefix = format!("{prefix}/schema/");
    let value_topic = format!("{prefix}/settings/value");
    let nested_topic = format!("{prefix}/settings/nested/leaf");
    timeout(Duration::from_secs(5), async {
        while !(saw_alive && saw_schema && saw_value && saw_nested) {
            let Some(inbound) = observer.poll().await.unwrap() else {
                continue;
            };
            assert!(
                has_utf8_payload_indicator(&inbound),
                "missing UTF-8 payload indicator on {}",
                inbound.topic()
            );
            saw_alive |= inbound.topic() == alive_topic;
            if inbound.topic() == alive_topic {
                assert!(
                    core::str::from_utf8(inbound.payload())
                        .unwrap()
                        .contains(r#""proto":1"#),
                    "missing MM2 proto in alive payload: {}",
                    core::str::from_utf8(inbound.payload()).unwrap()
                );
            }
            saw_schema |= inbound.topic().starts_with(&schema_prefix);
            saw_value |= inbound.topic() == value_topic;
            saw_nested |= inbound.topic() == nested_topic;
        }
    })
    .await
    .unwrap();

    let reply_topic = format!("{prefix}/reply");
    let response_props = [Property::ResponseTopic(&reply_topic)];
    let mut requester_session = Session::new(config());
    let mut requester =
        wait_session(&mut requester_session, connect_addr(addr).await.unwrap()).await;
    let topics = [TopicFilter::new(&reply_topic)
        .options(SubscriptionOptions::default().maximum_qos(QoS::AtLeastOnce))];
    requester.subscribe(&topics, &[]).await.unwrap();
    requester
        .publish(
            Publication::bytes(&format!("{prefix}/set/value"), b"9").properties(&response_props),
        )
        .await
        .unwrap();

    match timeout(
        Duration::from_secs(5),
        mm2.serve(&mut connection, &mut settings, |_| ()),
    )
    .await
    .unwrap()
    .unwrap()
    {
        Event::Unhandled(_) => panic!("unexpected app traffic"),
        Event::Changed(_) => {}
    }
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(inbound) = requester.poll().await.unwrap() {
                assert_eq!(inbound.topic(), reply_topic);
                assert!(has_utf8_payload_indicator(&inbound));
                break;
            }
        }
    })
    .await
    .unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            let Some(inbound) = observer.poll().await.unwrap() else {
                continue;
            };
            if inbound.topic() == value_topic {
                assert_eq!(user_property(&inbound, "auth"), Some(""));
                assert!(has_utf8_payload_indicator(&inbound));
                break;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(settings.value, 9);
}

#[tokio::test]
async fn mm2_set_stays_internal() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("prefix");
    let mut publisher_session = Session::new(config());
    let mut publisher =
        wait_session(&mut publisher_session, connect_addr(addr).await.unwrap()).await;
    let (mut mm2, mut session) = Miniconf::new(&prefix, config()).unwrap();
    let mut settings = Settings::default();
    let mut connection = connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    publisher
        .publish(Publication::bytes(&format!("{prefix}/set/value"), b"9"))
        .await
        .unwrap();

    match timeout(
        Duration::from_secs(5),
        mm2.serve(&mut connection, &mut settings, |_| ()),
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
async fn retained_load_applies_only_auth_leaf_values() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("retained-load");
    let mut seeder_session = Session::new(config());
    let mut seeder = wait_session(&mut seeder_session, connect_addr(addr).await.unwrap()).await;
    let auth = [Property::UserProperty("auth", "")];
    let op = seeder
        .publish(
            Publication::bytes(&format!("{prefix}/settings/value"), b"9")
                .properties(&auth)
                .qos(QoS::AtLeastOnce)
                .retain(),
        )
        .await
        .unwrap()
        .unwrap();
    wait_op(&mut seeder, op).await;
    let op = seeder
        .publish(
            Publication::bytes(&format!("{prefix}/settings/nested/leaf"), b"7")
                .qos(QoS::AtLeastOnce)
                .retain(),
        )
        .await
        .unwrap()
        .unwrap();
    wait_op(&mut seeder, op).await;
    let op = seeder
        .publish(
            Publication::bytes(&format!("{prefix}/settings/obsolete"), b"1")
                .properties(&auth)
                .qos(QoS::AtLeastOnce)
                .retain(),
        )
        .await
        .unwrap()
        .unwrap();
    wait_op(&mut seeder, op).await;

    let (mut mm2, mut session) = Miniconf::<Settings>::new(&prefix, config()).unwrap();
    let mut settings = Settings::default();
    let mut connection = wait_session(&mut session, connect_addr(addr).await.unwrap()).await;
    let mut load = LoadRetained::new();
    timeout(
        Duration::from_secs(5),
        load.run(&mut mm2, &mut connection, &mut settings),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(settings.value, 9);
    assert_eq!(settings.nested.leaf, 0);

    let mut startup = miniconf_mqtt::Startup::connected(&mut mm2);
    timeout(
        Duration::from_secs(5),
        startup.run(&mut mm2, &mut connection, &settings),
    )
    .await
    .unwrap()
    .unwrap();
}

#[tokio::test]
async fn service_accepts_no_auth_settings_compat_ingress() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("settings-compat");
    let mut publisher_session = Session::new(config());
    let mut publisher =
        wait_session(&mut publisher_session, connect_addr(addr).await.unwrap()).await;
    let (mut mm2, mut session) = Miniconf::new(&prefix, config()).unwrap();
    let mut settings = Settings::default();
    let mut connection = connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    let compat_filter = format!("{prefix}/settings/#");
    let topics = [TopicFilter::new(&compat_filter).options(
        SubscriptionOptions::default()
            .retain_behavior(RetainHandling::Never)
            .ignore_local_messages(),
    )];
    let op = connection.subscribe(&topics, &[]).await.unwrap();
    wait_op(&mut connection, op).await;

    publisher
        .publish(Publication::bytes(
            &format!("{prefix}/settings/value"),
            b"11",
        ))
        .await
        .unwrap();

    let mut service = Service::<4>::new();
    timeout(Duration::from_secs(5), async {
        loop {
            let Some(inbound) = connection.poll().await.unwrap() else {
                continue;
            };
            if inbound.topic() != format!("{prefix}/settings/value") {
                continue;
            }
            assert!(matches!(
                service.handle(&mut mm2, &mut settings, &inbound),
                ServiceEvent::Changed(_)
            ));
            while !service
                .step(&mut mm2, &mut connection, &settings)
                .await
                .unwrap()
            {
                let _ = connection.poll().await.unwrap();
            }
            break;
        }
    })
    .await
    .unwrap();

    assert_eq!(settings.value, 11);

    publisher
        .publish(Publication::bytes(
            &format!("{prefix}/settings/value"),
            b"bad",
        ))
        .await
        .unwrap();
    timeout(Duration::from_secs(5), async {
        loop {
            let Some(inbound) = connection.poll().await.unwrap() else {
                continue;
            };
            if inbound.topic() != format!("{prefix}/settings/value")
                || user_property(&inbound, "auth").is_some()
            {
                continue;
            }
            assert!(matches!(
                service.handle(&mut mm2, &mut settings, &inbound),
                ServiceEvent::Idle
            ));
            while !service
                .step(&mut mm2, &mut connection, &settings)
                .await
                .unwrap()
            {
                let _ = connection.poll().await.unwrap();
            }
            break;
        }
    })
    .await
    .unwrap();

    assert_eq!(settings.value, 11);
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
    let mut publisher_session = Session::new(config());
    let (mut mm2, mut session) = Miniconf::new(&prefix, config()).unwrap();
    let settings = Settings::default();

    let mut publisher =
        wait_session(&mut publisher_session, connect_addr(addr).await.unwrap()).await;
    let mut connection = connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    publisher
        .publish(
            Publication::bytes(&other_topic, b"hello")
                .retain()
                .qos(QoS::AtLeastOnce),
        )
        .await
        .unwrap();

    let topics = [TopicFilter::new(&other_topic)
        .options(SubscriptionOptions::default().maximum_qos(QoS::AtMostOnce))];
    connection.subscribe(&topics, &[]).await.unwrap();
    let marker = String::from("fn-once");
    match timeout(
        Duration::from_secs(5),
        mm2.serve(&mut connection, &mut Settings::default(), |message| {
            (
                marker,
                message.topic().to_owned(),
                message.payload().to_vec(),
            )
        }),
    )
    .await
    .unwrap()
    .unwrap()
    {
        Event::Unhandled((marker, topic, payload)) => {
            assert_eq!(marker, "fn-once");
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
    let (mut mm2, mut session) =
        Miniconf::<common::Settings>::new(&prefix, compact_config()).unwrap();
    let settings = common::Settings::new();

    let mut connection = timeout(
        Duration::from_secs(5),
        session.connect(connect_addr(addr).await.unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(
        connection.connect_event(),
        ConnectEvent::Connected
    ));

    let mut startup = miniconf_mqtt::Startup::new(&mut mm2, ConnectEvent::Connected);
    let mut retries = 0usize;
    let mut saw_non_quiescent = false;
    let mut saw_internal_progress = false;
    timeout(Duration::from_secs(5), async {
        while !startup
            .step(&mut mm2, &mut connection, &settings)
            .await
            .unwrap()
        {
            retries += 1;
            saw_non_quiescent |= !connection.session().is_publish_quiescent();
            saw_internal_progress |= connection.poll().await.unwrap().is_none();
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
    assert!(connection.session().is_publish_quiescent());
}

#[tokio::test]
async fn service_accepts_later_sets_while_earlier_response_is_pending() {
    init_host_logging();
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let prefix = unique("queue");
    let mut publisher_session = Session::new(config());
    let (mut mm2, mut session) = Miniconf::<Settings>::new(&prefix, config()).unwrap();
    let mut settings = Settings::default();
    let mut service = Service::<4>::new();

    let mut connection = connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    let mut publisher =
        wait_session(&mut publisher_session, connect_addr(addr).await.unwrap()).await;
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
            let Some(inbound) = connection.poll().await.unwrap() else {
                continue;
            };
            match service.handle(&mut mm2, &mut settings, &inbound) {
                ServiceEvent::Unhandled => panic!("unexpected app traffic"),
                ServiceEvent::Idle | ServiceEvent::Busy => {}
                ServiceEvent::Changed(_) => {
                    accepted += 1;
                    saw_backlog |= service.len() > 1;
                }
            }
        }

        while !service.is_empty() {
            if service
                .step(&mut mm2, &mut connection, &settings)
                .await
                .unwrap()
            {
                break;
            }
            let _ = connection.poll().await.unwrap();
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
    let mut publisher_session = Session::new(config());
    let (mut mm2, mut session) = Miniconf::<Settings>::new(&prefix, config()).unwrap();
    let mut settings = Settings::default();
    let mut service = Service::<1>::new();

    let mut connection = connect_mm2(
        &mut mm2,
        &mut session,
        &settings,
        connect_addr(addr).await.unwrap(),
    )
    .await;

    let mut publisher =
        wait_session(&mut publisher_session, connect_addr(addr).await.unwrap()).await;
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
        let inbound = timeout(Duration::from_secs(5), connection.poll())
            .await
            .unwrap()
            .unwrap();
        if let Some(inbound) = inbound {
            break inbound;
        }
    };
    assert!(matches!(
        service.handle(&mut mm2, &mut settings, &first),
        ServiceEvent::Changed(_)
    ));

    let second = loop {
        let inbound = timeout(Duration::from_secs(5), connection.poll())
            .await
            .unwrap()
            .unwrap();
        if let Some(inbound) = inbound {
            break inbound;
        }
    };
    assert!(matches!(
        service.handle(&mut mm2, &mut settings, &second),
        ServiceEvent::Busy
    ));

    assert_eq!(settings.value, 9);
    assert_eq!(settings.nested.leaf, 0);
}
