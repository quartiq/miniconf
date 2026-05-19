extern crate std;

use crate::{
    MAX_TOPIC_LENGTH, Miniconf,
    schema::{SchemaDefs, serialize_schema_page},
};
use embedded_io_adapters::tokio_1::FromTokio;
use miniconf::{Tree, TreeSchema};
use minimq::{ConfigBuilder, ConfigError};
use std::sync::OnceLock;
use tokio::net::TcpStream;

#[derive(Tree)]
struct Tiny {
    value: u8,
}

#[derive(Tree, Default)]
struct Nested {
    leaf: u8,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(meta(role = "selector"))]
    value: u8,
    nested: Nested,
}

fn init_host_logging() {
    static HOST_LOGGING: OnceLock<()> = OnceLock::new();

    HOST_LOGGING.get_or_init(|| {
        env_logger::builder().is_test(true).try_init().unwrap();
        defmt2log::init_from_current_exe();
    });
}

#[test]
fn constructor_rejects_long_prefix() {
    init_host_logging();
    let prefix = "x".repeat(MAX_TOPIC_LENGTH);
    let mut buffer = [0u8; 1024];
    let client = Miniconf::<Tiny>::new::<FromTokio<TcpStream>>(
        &prefix,
        ConfigBuilder::from_buffer(&mut buffer, 1024).unwrap(),
    );
    assert!(matches!(client, Err(ConfigError::InvalidConfig)));
}

#[test]
fn schema_pages_match_golden_fixture() {
    init_host_logging();
    let mut payload = [0u8; 1024];
    let defs = SchemaDefs::new(Settings::SCHEMA).unwrap();
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
    init_host_logging();
    let defs = SchemaDefs::new(Settings::SCHEMA).unwrap();
    assert_eq!(defs.root(), Some(Settings::SCHEMA));
}
