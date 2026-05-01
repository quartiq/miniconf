use crate::{
    MAX_TOPIC_LENGTH, Miniconf,
    schema::{SchemaDefs, serialize_schema_page},
};
use embedded_io_async::{ErrorKind, ErrorType, Read, Write};
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

#[test]
fn constructor_rejects_long_prefix() {
    let prefix = "x".repeat(MAX_TOPIC_LENGTH);
    let mut buffer = [0u8; 1024];
    let client = Miniconf::<Tiny>::new::<DummyConnection>(
        &prefix,
        minimq::ConfigBuilder::from_buffer(&mut buffer, 1024).unwrap(),
    );
    assert!(matches!(client, Err(ProtocolError::BufferSize)));
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
