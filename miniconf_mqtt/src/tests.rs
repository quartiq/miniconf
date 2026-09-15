extern crate std;

use crate::{MAX_SCHEMA_DEFS, MAX_TOPIC_LENGTH, Miniconf};
use miniconf::{
    Tree, TreeSchema,
    compact_schema::{SchemaDefs, serialize_schema_page},
};
use minimq::{ConfigBuilder, ConfigError};
use std::sync::OnceLock;

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
    let client = Miniconf::<Tiny>::new(
        &prefix,
        ConfigBuilder::from_buffer(&mut buffer, 1024).unwrap(),
    );
    assert!(matches!(client, Err(ConfigError::InvalidConfig)));
}

#[test]
fn schema_pages_match_golden_fixture() {
    init_host_logging();
    let mut payload = [0u8; 1024];
    let defs = SchemaDefs::<MAX_SCHEMA_DEFS>::new(Settings::SCHEMA).unwrap();
    let page = serialize_schema_page(&defs, 0, &mut payload).unwrap();
    assert_eq!(page.count, 3);
    let normalized = core::str::from_utf8(&payload[..page.len])
        .unwrap()
        .replace(r#"{"s":{"ty":"u8"}}"#, "{}");
    assert_eq!(
        normalized,
        include_str!("../../fixtures/compact-schema.ndjson")
    );
}

#[test]
fn schema_defs_keep_root_last() {
    init_host_logging();
    let defs = SchemaDefs::<MAX_SCHEMA_DEFS>::new(Settings::SCHEMA).unwrap();
    assert_eq!(defs.root(), Some(Settings::SCHEMA));
}

#[tokio::test]
async fn startup_rejects_a_qos_zero_broker() {
    use core::{
        pin::Pin,
        task::{Context, Poll},
    };
    use embedded_io_adapters::tokio_1::FromTokio;
    use std::io;
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    // A successful MQTT v5 CONNACK advertising Maximum QoS = 0.
    struct Broker(&'static [u8]);
    impl AsyncRead for Broker {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.0.is_empty() {
                return Poll::Pending;
            }
            let count = buf.remaining().min(self.0.len());
            buf.put_slice(&self.0[..count]);
            self.0 = &self.0[count..];
            Poll::Ready(Ok(()))
        }
    }
    impl AsyncWrite for Broker {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buf.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    init_host_logging();
    for downgrade in [false, true] {
        let mut buffer = [0; 1024];
        let config = ConfigBuilder::from_buffer(&mut buffer, 128)
            .unwrap()
            .client_id("qos-test")
            .unwrap();
        let config = if downgrade {
            config.autodowngrade_qos()
        } else {
            config
        };
        let (mut miniconf, mut session) = Miniconf::<Tiny>::new("test/qos", config).unwrap();
        let mut connection = session
            .connect(FromTokio::new(Broker(&[0x20, 5, 0, 0, 2, 0x24, 0])))
            .await
            .unwrap();
        let mut startup = crate::Startup::new(&mut miniconf, connection.connect_event());
        assert!(matches!(
            startup
                .step(&mut miniconf, &mut connection, &Tiny { value: 0 })
                .await,
            Err(crate::Error::Mqtt(minimq::Error::InvalidRequest))
        ));
        assert!(!miniconf.startup_complete);
    }
}
