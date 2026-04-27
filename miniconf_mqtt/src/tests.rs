use crate::{
    MAX_TOPIC_LENGTH, MqttClient,
    message::{Action, ReplyTarget, ResponseCode, format_slice},
    schema::{SchemaDefs, serialize_schema_page},
};
use embedded_io_async::{ErrorKind, ErrorType, Read, ReadReady, Write, WriteReady};
use miniconf::{Tree, TreeSchema};
use minimq::ProtocolError;

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
    #[tree(meta(role = "selector"))]
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

impl ReadReady for DummyConnection {
    fn read_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

impl WriteReady for DummyConnection {
    fn write_ready(&mut self) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

#[test]
fn constructor_rejects_long_prefix() {
    let mut buffer = [0u8; 1024];
    let prefix = "x".repeat(MAX_TOPIC_LENGTH);

    let client = MqttClient::<Tiny, DummyConnection>::new(
        &prefix,
        minimq::ConfigBuilder::from_buffer(&mut buffer, 1024).unwrap(),
    );

    assert!(matches!(client, Err(ProtocolError::BufferSize)));
}

#[test]
fn plan_set_marks_changed_with_explicit_reply() {
    let mut settings = TreeSettings::default();
    let reply = ReplyTarget::new("test/id/response", None).unwrap();

    match MqttClient::<TreeSettings, DummyConnection>::plan_publish(
        "test/id",
        &mut settings,
        "test/id/set/value",
        b"42",
        Some(reply),
    ) {
        Action::PublishSet { reply, depth, .. } => {
            assert!(reply.is_some());
            assert_eq!(depth, 1);
            assert_eq!(settings.value, 42);
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_set_without_response_topic_is_fire_and_forget() {
    let mut settings = TreeSettings::default();
    match MqttClient::<TreeSettings, DummyConnection>::plan_publish(
        "test/id",
        &mut settings,
        "test/id/set/value",
        b"7",
        None,
    ) {
        Action::PublishSet { reply, depth, .. } => {
            assert!(reply.is_none());
            assert_eq!(depth, 1);
            assert_eq!(settings.value, 7);
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_empty_payload_is_ignored() {
    let mut settings = TreeSettings::default();
    let reply = ReplyTarget::new("test/id/response", None).unwrap();
    assert!(matches!(
        MqttClient::<TreeSettings, DummyConnection>::plan_publish(
            "test/id",
            &mut settings,
            "test/id/set/value",
            b"",
            Some(reply),
        ),
        Action::None(crate::client::Change::Unchanged)
    ));
    assert_eq!(settings.value, 0);
}

#[test]
fn plan_internal_set_path_is_rejected() {
    let mut settings = TreeSettings::default();
    let reply = ReplyTarget::new("test/id/response", None).unwrap();

    match MqttClient::<TreeSettings, DummyConnection>::plan_publish(
        "test/id",
        &mut settings,
        "test/id/set/nested",
        b"{}",
        Some(reply),
    ) {
        Action::Reply {
            state,
            reply,
            code,
            body,
        } => {
            assert_eq!(state, crate::client::Change::Unchanged);
            assert!(reply.is_some());
            assert_eq!(code, ResponseCode::Error);
            let mut text = [0u8; 128];
            let len = format_slice(&body, &mut text).unwrap();
            assert_eq!(
                core::str::from_utf8(&text[..len]).unwrap(),
                "Path does not resolve to a leaf"
            );
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn plan_missing_path_is_rejected() {
    let mut settings = TreeSettings::default();
    let reply = ReplyTarget::new("test/id/response", None).unwrap();

    match MqttClient::<TreeSettings, DummyConnection>::plan_publish(
        "test/id",
        &mut settings,
        "test/id/set/missing",
        b"1",
        Some(reply),
    ) {
        Action::Reply {
            state,
            reply,
            code,
            body,
        } => {
            assert_eq!(state, crate::client::Change::Unchanged);
            assert!(reply.is_some());
            assert_eq!(code, ResponseCode::Error);
            let mut text = [0u8; 128];
            let len = format_slice(&body, &mut text).unwrap();
            assert!(
                core::str::from_utf8(&text[..len])
                    .unwrap()
                    .contains("Key not found")
            );
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[test]
fn oversized_response_topic_is_rejected_early() {
    let response = "x".repeat(MAX_TOPIC_LENGTH + 1);
    assert!(matches!(
        ReplyTarget::new(&response, None),
        Err(ProtocolError::BufferSize)
    ));
}

#[test]
fn schema_pages_match_golden_fixture() {
    let mut payload = [0u8; 1024];
    let defs = SchemaDefs::new(TreeSettings::SCHEMA).unwrap();
    let page = serialize_schema_page(&defs, 0, &mut payload).unwrap();
    assert_eq!(page.count, 3);
    let normalized = core::str::from_utf8(&payload[..page.len])
        .unwrap()
        .replace(r#"{"s":{"ty":"u8"}}"#, "{}");
    assert_eq!(
        normalized,
        include_str!("../../testdata/compact-schema/fixture.ndjson")
    );
}

#[test]
fn schema_defs_keep_root_last() {
    let defs = SchemaDefs::new(TreeSettings::SCHEMA).unwrap();
    assert_eq!(defs.root(), Some(TreeSettings::SCHEMA));
}

#[cfg(feature = "compat-settings-ingress")]
#[test]
fn compatibility_settings_ingress_is_accepted() {
    let mut settings = TreeSettings::default();
    match MqttClient::<TreeSettings, DummyConnection>::plan_publish(
        "test/id",
        &mut settings,
        "test/id/settings/value",
        b"11",
        None,
    ) {
        Action::PublishSet { reply, depth, .. } => {
            assert!(reply.is_none());
            assert_eq!(depth, 1);
            assert_eq!(settings.value, 11);
        }
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}

#[cfg(feature = "compat-settings-ingress")]
#[test]
fn compatibility_settings_invalid_value_triggers_override() {
    let mut settings = TreeSettings::default();
    match MqttClient::<TreeSettings, DummyConnection>::plan_publish(
        "test/id",
        &mut settings,
        "test/id/settings/value",
        b"oops",
        None,
    ) {
        Action::OverrideSet { depth, .. } => assert_eq!(depth, 1),
        other => panic!("unexpected action: {}", core::any::type_name_of_val(&other)),
    }
}
