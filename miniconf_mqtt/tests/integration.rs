use miniconf::Tree;
use miniconf_mqtt::{Event, MqttClient, minimq};
use minimq::{
    Broker, Event as SessionEvent, Publication, QoS, Session,
    transport::TcpConnector,
    types::{SubscriptionOptions, TopicFilter},
};
use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};
use std_embedded_nal_async::Stack;

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

async fn wait_client(
    client: &mut MqttClient<'_, Settings, TcpConnector<Stack>>,
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

async fn wait_session(session: &mut Session<'_, '_, TcpConnector<Stack>>) {
    for _ in 0..200 {
        match session.poll().await.unwrap() {
            SessionEvent::Connected | SessionEvent::Reconnected => return,
            SessionEvent::Idle => {}
            SessionEvent::Inbound(_) => panic!("unexpected inbound publish on publisher"),
        }
    }
    panic!("timed out waiting for publisher session");
}

#[tokio::test]
async fn mm2_set_stays_internal() {
    let Some(addr) = broker_addr() else {
        eprintln!("skipping broker-backed test; set {BROKER_ADDR_ENV}=host:port");
        return;
    };

    let connector = TcpConnector::new(Stack::default());
    let prefix = unique("prefix");
    let mut publisher = Session::new(config(addr.into(), &unique("pub")), &connector);
    let mut client =
        MqttClient::<Settings, _>::new(&prefix, &connector, config(addr.into(), &unique("mm2")))
            .unwrap();
    let mut settings = Settings::default();

    let _ = wait_client(
        &mut client,
        &mut settings,
        |_| {},
        |event| matches!(event, Event::Connected | Event::Reconnected),
    )
    .await;

    wait_session(&mut publisher).await;
    publisher
        .publish(Publication::new(&format!("{prefix}/set/value"), b"9"))
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

    let connector = TcpConnector::new(Stack::default());
    let prefix = unique("prefix");
    let other_topic = format!("{prefix}/rpc/in");
    let mut publisher = Session::new(config(addr.into(), &unique("pub")), &connector);
    let mut client =
        MqttClient::<Settings, _>::new(&prefix, &connector, config(addr.into(), &unique("sub")))
            .unwrap();
    let mut settings = Settings::default();

    let _ = wait_client(
        &mut client,
        &mut settings,
        |_| {},
        |event| matches!(event, Event::Connected),
    )
    .await;
    let topics = [TopicFilter::new(&other_topic)
        .options(SubscriptionOptions::default().maximum_qos(QoS::AtMostOnce))];
    client.subscribe(&topics, &[]).await.unwrap();
    let _ = wait_client(&mut client, &mut settings, |_| {}, |_| true).await;

    wait_session(&mut publisher).await;
    publisher
        .publish(Publication::new(&other_topic, b"hello"))
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
