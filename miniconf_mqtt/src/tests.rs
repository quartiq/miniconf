use crate::{MqttClient, State, client::Action, pending::Pending};
use miniconf::{Tree, TreeSchema};
use minimq::{
    Broker, BufferLayout, InboundPublish, Property, ProtocolError, QoS, Retain,
    embedded_io_async::{ErrorKind, ErrorType, Read, Write},
    transport::Connector,
    types::{Properties, Utf8String},
};

#[derive(Tree)]
struct Tiny {
    value: u8,
}

#[derive(Tree, Default)]
struct Nested {
    leaf: u8,
}

#[derive(Tree, Default)]
struct TreeSettings {
    value: u8,
    nested: Nested,
}

#[derive(Default)]
struct DummyConnection;

impl ErrorType for DummyConnection {
    type Error = ErrorKind;
}

impl Read for DummyConnection {
    async fn read(&mut self, _buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(0)
    }
}

impl Write for DummyConnection {
    async fn write(&mut self, _buf: &[u8]) -> Result<usize, Self::Error> {
        Ok(0)
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct DummyConnector;

impl Connector for DummyConnector {
    type Error = ErrorKind;
    type Connection<'a> = DummyConnection;

    async fn connect<'a>(
        &'a self,
        _broker: &Broker<'_>,
    ) -> Result<Self::Connection<'a>, minimq::Error> {
        Ok(DummyConnection)
    }
}

#[test]
fn constructor_rejects_long_prefix() {
    let mut buffer = [0u8; 1024];
    let broker: Broker<'_> = "127.0.0.1:1883"
        .parse::<core::net::SocketAddr>()
        .unwrap()
        .into();
    const MAX_DEPTH: usize = Tiny::SCHEMA.shape().max_depth;
    let prefix = "x".repeat(crate::MAX_TOPIC_LENGTH);

    let client = MqttClient::<Tiny, _, MAX_DEPTH>::new(
        &prefix,
        &DummyConnector,
        minimq::ConfigBuilder::from_buffer_layout(
            broker,
            &mut buffer,
            BufferLayout { rx: 256, tx: 768 },
        )
        .unwrap(),
    );

    assert!(matches!(client, Err(ProtocolError::BufferSize)));
}

#[test]
fn plan_leaf_get() {
    let mut settings = TreeSettings::default();
    let message = InboundPublish {
        topic: "test/id/settings/value",
        payload: b"",
        properties: Properties::Slice(&[]),
        retain: Retain::NotRetained,
        qos: QoS::AtMostOnce,
    };

    match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
        "test/id",
        false,
        &mut settings,
        &message,
    ) {
        Action::ReplyLeaf { depth, .. } => {
            assert_eq!(depth, 1);
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_internal_get_without_response_topic_starts_dump() {
    let mut settings = TreeSettings::default();
    let message = InboundPublish {
        topic: "test/id/settings/nested",
        payload: b"",
        properties: Properties::Slice(&[]),
        retain: Retain::NotRetained,
        qos: QoS::AtMostOnce,
    };

    match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
        "test/id",
        false,
        &mut settings,
        &message,
    ) {
        Action::SetPending {
            pending: Pending::Dump { iter },
        } => assert_eq!(iter.root(), 1),
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_internal_get_with_response_topic_starts_list() {
    let mut settings = TreeSettings::default();
    let props = [Property::ResponseTopic(Utf8String("test/id/response"))];
    let message = InboundPublish {
        topic: "test/id/settings/nested",
        payload: b"",
        properties: Properties::Slice(&props),
        retain: Retain::NotRetained,
        qos: QoS::AtMostOnce,
    };

    match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
        "test/id",
        false,
        &mut settings,
        &message,
    ) {
        Action::SetPending {
            pending: Pending::List { iter, .. },
        } => assert_eq!(iter.root(), 1),
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_internal_get_with_oversized_response_topic_is_rejected() {
    let mut settings = TreeSettings::default();
    let response = "x".repeat(crate::MAX_TOPIC_LENGTH + 1);
    let props = [Property::ResponseTopic(Utf8String(&response))];
    let message = InboundPublish {
        topic: "test/id/settings/nested",
        payload: b"",
        properties: Properties::Slice(&props),
        retain: Retain::NotRetained,
        qos: QoS::AtMostOnce,
    };

    assert!(matches!(
        MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ),
        Action::None(State::Unchanged)
    ));
}

#[test]
fn plan_set_marks_changed() {
    let mut settings = TreeSettings::default();
    let props = [Property::ResponseTopic(Utf8String("test/id/response"))];
    let message = InboundPublish {
        topic: "test/id/settings/value",
        payload: b"42",
        properties: Properties::Slice(&props),
        retain: Retain::NotRetained,
        qos: QoS::AtMostOnce,
    };

    match MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
        "test/id",
        false,
        &mut settings,
        &message,
    ) {
        Action::ReplyText { state, code, .. } => {
            assert_eq!(state, State::Changed);
            assert_eq!(code, crate::protocol::ResponseCode::Ok);
            assert_eq!(settings.value, 42);
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_set_with_oversized_response_topic_is_rejected() {
    let mut settings = TreeSettings::default();
    let response = "x".repeat(crate::MAX_TOPIC_LENGTH + 1);
    let props = [Property::ResponseTopic(Utf8String(&response))];
    let message = InboundPublish {
        topic: "test/id/settings/value",
        payload: b"42",
        properties: Properties::Slice(&props),
        retain: Retain::NotRetained,
        qos: QoS::AtMostOnce,
    };

    assert!(matches!(
        MqttClient::<TreeSettings, DummyConnector, 2>::plan_request(
            "test/id",
            false,
            &mut settings,
            &message,
        ),
        Action::None(State::Unchanged)
    ));
    assert_eq!(settings.value, 0);
}
