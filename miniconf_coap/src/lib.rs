#![no_std]
#![warn(missing_docs)]

//! Serve selected `miniconf` trees as CoAP resources.
//!
//! The crate is deliberately sessionless. Applications pass caller-owned settings into
//! cooperative request handlers, and keep ownership of CoAP sockets, message IDs, tokens, routing,
//! retransmission, and unrelated resources.

use miniconf::Indices;

/// Maximum Miniconf tree depth supported by `miniconf_coap`.
pub const MAX_DEPTH: usize = 12;

/// Maximum bytes in a captured rooted CoAP URI path.
pub const MAX_URI_PATH_LENGTH: usize = 256;

/// Maximum request payload bytes and response scratch bytes used by the optional `coap-handler` adapter.
pub const MAX_HANDLER_PAYLOAD_LENGTH: usize = 512;

/// Maximum compact schema definitions served by `miniconf_coap`.
pub const MAX_SCHEMA_DEFS: usize = 64;

/// Exact changed leaf indices produced by a successful `PUT`.
pub type ChangedKey = Indices<[usize; MAX_DEPTH]>;

#[cfg(any(feature = "json-core", feature = "cbor", feature = "coap-handler"))]
pub(crate) const fn content_format(name: &str) -> u16 {
    match coap_numbers::content_format::from_str(name) {
        Some(value) => value,
        None => panic!("unknown CoAP content format"),
    }
}

#[cfg(feature = "coap-handler")]
mod handler;
mod message;
#[cfg(feature = "json-core")]
mod schema;
mod value;

#[cfg(all(feature = "coap-handler", feature = "cbor"))]
pub use handler::ConstPathCborCoapHandler;
#[cfg(all(feature = "coap-handler", feature = "json-core"))]
pub use handler::ConstPathJsonCoapHandler;
#[cfg(feature = "coap-handler")]
pub use handler::MiniconfHandler;
#[cfg(all(feature = "coap-handler", feature = "json-core"))]
pub use handler::MiniconfSchemaHandler;
pub use message::{Error, Operation, Outcome, Problem, RequestParts, Response};
#[cfg(feature = "json-core")]
pub use schema::SchemaHandler;
pub use value::ValueHandler;
#[cfg(feature = "cbor")]
pub use value::{ConstPathCbor, ConstPathCborHandler};
#[cfg(feature = "json-core")]
pub use value::{ConstPathJson, ConstPathJsonHandler};

#[cfg(feature = "coap-handler")]
pub(crate) use message::{Accepts, InvalidOption, UriPath};

#[cfg(all(test, feature = "json-core"))]
mod tests {
    extern crate std;

    use core::convert::Infallible;

    use coap_message::{MessageOption as _, MinimalWritableMessage, ReadableMessage};
    use coap_numbers::{code, option};
    use miniconf::Tree;
    use std::{sync::OnceLock, vec::Vec};

    use super::*;

    #[derive(Tree)]
    struct Settings {
        hidden: bool,
        number: u32,
        #[tree(with = label)]
        label: heapless::String<16>,
        visible: Option<Visible>,
    }

    #[derive(Tree)]
    struct Visible {
        value: u8,
    }

    impl Default for Settings {
        fn default() -> Self {
            Self {
                hidden: false,
                number: 7,
                label: "demo".try_into().unwrap(),
                visible: Some(Visible { value: 9 }),
            }
        }
    }

    fn init_host_logging() {
        static HOST_LOGGING: OnceLock<()> = OnceLock::new();

        HOST_LOGGING.get_or_init(|| {
            env_logger::builder().is_test(true).try_init().unwrap();
            defmt2log::init_from_current_exe();
        });
    }

    mod label {
        use miniconf::{Keys, SerdeError, ValueError, leaf};
        use serde::{Deserializer, Serializer};

        pub use leaf::{mut_any_by_key, probe_by_key, ref_any_by_key, schema};

        pub fn serialize_by_key<S: Serializer>(
            value: &heapless::String<16>,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerdeError<S::Error>> {
            leaf::serialize_by_key(value, keys, ser)
        }

        pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
            value: &mut heapless::String<16>,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerdeError<D::Error>> {
            let mut next = value.clone();
            leaf::deserialize_by_key(&mut next, keys, de)?;
            if next.contains('<') {
                return Err(ValueError::Access("bad label").into());
            }
            *value = next;
            Ok(())
        }
    }

    fn request(
        code: u8,
        path: &[&str],
        content_format: Option<u16>,
        payload: &'static [u8],
    ) -> RequestParts<'static> {
        RequestParts::new(code, path, None, content_format, payload).unwrap()
    }

    #[derive(Debug, Clone)]
    struct TestOption {
        number: u16,
        value: Vec<u8>,
    }

    impl coap_message::MessageOption for &TestOption {
        fn number(&self) -> u16 {
            self.number
        }

        fn value(&self) -> &[u8] {
            &self.value
        }
    }

    #[derive(Debug, Default, Clone)]
    struct TestMessage {
        code: u8,
        options: Vec<TestOption>,
        payload: Vec<u8>,
    }

    impl TestMessage {
        fn new(code: u8) -> Self {
            Self {
                code,
                options: Vec::new(),
                payload: Vec::new(),
            }
        }

        fn str_option(mut self, number: u16, value: &str) -> Self {
            self.options.push(TestOption {
                number,
                value: value.as_bytes().to_vec(),
            });
            self
        }

        fn option(mut self, number: u16, value: &[u8]) -> Self {
            self.options.push(TestOption {
                number,
                value: value.to_vec(),
            });
            self
        }

        fn uint_option(mut self, number: u16, value: u16) -> Self {
            self.options.push(TestOption {
                number,
                value: uint_vec(value),
            });
            self
        }

        fn with_payload(mut self, payload: &[u8]) -> Self {
            self.payload = payload.to_vec();
            self
        }
    }

    impl ReadableMessage for TestMessage {
        type Code = u8;
        type MessageOption<'a> = &'a TestOption;
        type OptionsIter<'a> = core::slice::Iter<'a, TestOption>;

        fn code(&self) -> Self::Code {
            self.code
        }

        fn options(&self) -> Self::OptionsIter<'_> {
            self.options.iter()
        }

        fn payload(&self) -> &[u8] {
            &self.payload
        }
    }

    impl MinimalWritableMessage for TestMessage {
        type AddOptionError = Infallible;
        type Code = u8;
        type OptionNumber = u16;
        type SetPayloadError = Infallible;
        type UnionError = Infallible;

        fn set_code(&mut self, code: Self::Code) {
            self.code = code;
        }

        fn add_option(
            &mut self,
            number: Self::OptionNumber,
            value: &[u8],
        ) -> Result<(), Self::AddOptionError> {
            self.options.push(TestOption {
                number,
                value: value.to_vec(),
            });
            Ok(())
        }

        fn set_payload(&mut self, data: &[u8]) -> Result<(), Self::SetPayloadError> {
            self.payload = data.to_vec();
            Ok(())
        }
    }

    fn uint_vec(value: u16) -> Vec<u8> {
        match value {
            0 => Vec::new(),
            1..=0xff => [value as u8].to_vec(),
            _ => value.to_be_bytes().to_vec(),
        }
    }

    fn route<'a>(
        request: &RequestParts<'_>,
        settings: &mut Settings,
        response_buf: &'a mut [u8],
    ) -> Outcome<'a> {
        let schema = SchemaHandler::new("/schema");
        let values = ConstPathJsonHandler::const_path_json("/settings");

        match request.path() {
            "/schema" => return schema.handle::<Settings>(request, response_buf),
            "/settings" => return values.handle(request, settings, response_buf),
            path if path.starts_with("/settings/") => {
                return values.handle(request, settings, response_buf);
            }
            _ => {}
        }
        if request.path() == "/status" {
            return Outcome::Handled(Response {
                code: code::CONTENT,
                content_format: Some(content_format("application/json")),
                payload: br#"{"ok":true}"#,
            });
        }
        Outcome::Unhandled
    }

    #[test]
    fn get_and_put_json_leaf() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let req = request(code::GET, &["settings", "number"], None, b"");
        let out = handler.handle(&req, &mut settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(
            response.content_format,
            Some(content_format("application/json"))
        );
        assert_eq!(response.payload, b"7");

        let mut response = [0; 128];
        let req = request(
            code::PUT,
            &["settings", "number"],
            Some(content_format("application/json")),
            b"12",
        );
        let out = handler.handle(&req, &mut settings, &mut response);
        assert!(matches!(out, Outcome::Changed { .. }));
        assert_eq!(settings.number, 12);
    }

    #[test]
    fn absent_not_found_and_too_long_stay_distinct() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings {
            visible: None,
            ..Default::default()
        };

        let mut response = [0; 128];
        let req = request(code::GET, &["settings", "visible", "value"], None, b"");
        let out = handler.handle(&req, &mut settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::CONFLICT);
        assert_eq!(response.payload, br#"{"kind":"absent","depth":2}"#);

        let mut response = [0; 128];
        let req = request(code::GET, &["settings", "missing"], None, b"");
        let out = handler.handle(&req, &mut settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::NOT_FOUND);
        assert_eq!(response.payload, br#"{"kind":"not_found","depth":0}"#);

        let mut response = [0; 128];
        let req = request(code::GET, &["settings", "number", "extra"], None, b"");
        let out = handler.handle(&req, &mut settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::NOT_FOUND);
        assert_eq!(response.payload, br#"{"kind":"too_long","depth":1}"#);
    }

    #[test]
    fn access_write_maps_to_unprocessable_entity() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let req = request(
            code::PUT,
            &["settings", "label"],
            Some(content_format("application/json")),
            br#""bad<label""#,
        );
        let out = handler.handle(&req, &mut settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::UNPROCESSABLE_ENTITY);
        assert_eq!(
            response.payload,
            br#"{"kind":"access","op":"write","message":"bad label"}"#
        );
    }

    #[test]
    fn schema_route_is_separate() {
        init_host_logging();
        let handler = SchemaHandler::new("/schema");
        let mut response = [0; 512];
        let req = request(code::GET, &["schema"], None, b"");
        let out = handler.handle::<Settings>(&req, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(
            response.content_format,
            Some(content_format("text/plain; charset=utf-8"))
        );
        assert!(response.payload.starts_with(b"{"));
    }

    #[test]
    fn schema_route_is_paged() {
        init_host_logging();
        let handler = SchemaHandler::new("/schema");
        let mut response = [0; 512];
        let req = request(code::GET, &["schema", "0"], None, b"");
        let out = handler.handle::<Settings>(&req, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(
            response.content_format,
            Some(content_format("text/plain; charset=utf-8"))
        );
        assert!(response.payload.starts_with(b"{"));

        let mut response = [0; 512];
        let req = request(code::GET, &["schema", "99"], None, b"");
        let out = handler.handle::<Settings>(&req, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::NOT_FOUND);
        assert_eq!(response.payload, br#"{"kind":"not_found","depth":1}"#);
    }

    #[test]
    fn get_and_put_leaf() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let req = request(code::GET, &["settings", "number"], None, b"");
        let outcome = handler.handle(&req, &mut settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.payload, b"7");

        let mut response = [0; 128];
        let req = request(
            code::PUT,
            &["settings", "number"],
            Some(content_format("application/json")),
            b"14",
        );
        assert!(matches!(
            handler.handle(&req, &mut settings, &mut response),
            Outcome::Changed { .. }
        ));
        assert_eq!(settings.number, 14);
    }

    #[test]
    fn repeated_accept_options_preserve_json_match() {
        init_host_logging();
        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::ACCEPT, 0)
            .uint_option(option::ACCEPT, content_format("application/json"));
        let request = RequestParts::from_message(&request).unwrap();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let outcome = handler.handle(&request, &mut settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(response.payload, b"7");

        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::ACCEPT, 0);
        let request = RequestParts::from_message(&request).unwrap();
        let mut response = [0; 128];
        let outcome = handler.handle(&request, &mut settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::NOT_ACCEPTABLE);

        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "schema")
            .uint_option(option::ACCEPT, 0)
            .uint_option(option::ACCEPT, content_format("application/json"));
        let request = RequestParts::from_message(&request).unwrap();
        let handler = SchemaHandler::new("/schema");
        let mut response = [0; 512];
        let outcome = handler.handle::<Settings>(&request, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(
            response.content_format,
            Some(content_format("text/plain; charset=utf-8"))
        );
    }

    #[test]
    fn accept_overflow_is_checked_at_representation_match() {
        init_host_logging();
        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::ACCEPT, content_format("application/json"))
            .uint_option(option::ACCEPT, 0)
            .uint_option(option::ACCEPT, 1)
            .uint_option(option::ACCEPT, 2)
            .uint_option(option::ACCEPT, 3);
        let request = RequestParts::from_message(&request).unwrap();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];
        let outcome = handler.handle(&request, &mut settings, &mut response);
        assert_eq!(outcome.response().unwrap().code, code::CONTENT);

        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::ACCEPT, 0)
            .uint_option(option::ACCEPT, 1)
            .uint_option(option::ACCEPT, 2)
            .uint_option(option::ACCEPT, 3)
            .uint_option(option::ACCEPT, 4);
        let request = RequestParts::from_message(&request).unwrap();
        let mut response = [0; 128];
        let outcome = handler.handle(&request, &mut settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::BAD_OPTION);
        assert_eq!(response.payload, br#"{"kind":"too_many_accept_options"}"#);
    }

    #[test]
    fn malformed_content_format_is_bad_request() {
        init_host_logging();
        let request = TestMessage::new(code::PUT)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .option(option::CONTENT_FORMAT, &[0, 0, 50])
            .with_payload(b"18");
        let err = RequestParts::from_message(&request).unwrap_err();
        assert_eq!(err.code, code::BAD_REQUEST);
        assert_eq!(err.problem, Problem::InvalidContentFormat);
    }

    #[test]
    fn duplicate_content_format_is_bad_request_on_matched_route() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let request = TestMessage::new(code::PUT)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::CONTENT_FORMAT, content_format("application/json"))
            .uint_option(option::CONTENT_FORMAT, content_format("application/json"))
            .with_payload(b"12");
        let request = RequestParts::from_message(&request).unwrap();
        let mut response = [0; 128];
        let outcome = handler.handle(&request, &mut settings, &mut response);
        let response = outcome.response().unwrap();

        assert_eq!(response.code, code::BAD_REQUEST);
        assert_eq!(response.payload, br#"{"kind":"duplicate_content_format"}"#);
        assert_eq!(settings.number, 7);
    }

    #[test]
    fn unknown_critical_option_is_bad_option_on_matched_route() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .option(99, b"critical");
        let request = RequestParts::from_message(&request).unwrap();
        let mut response = [0; 128];
        let outcome = handler.handle(&request, &mut settings, &mut response);
        let response = outcome.response().unwrap();

        assert_eq!(response.code, code::BAD_OPTION);
        assert_eq!(
            response.payload,
            br#"{"kind":"unknown_critical_option","number":99}"#
        );
    }

    #[test]
    fn get_serialization_failures_are_not_bad_payload() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 0];
        let req = request(code::GET, &["settings", "number"], None, b"");
        let outcome = handler.handle(&req, &mut settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::INTERNAL_SERVER_ERROR);
        assert_eq!(response.payload, b"");
    }

    #[test]
    fn long_problem_message_preserves_json_body() {
        let err = Error::new(
            code::UNPROCESSABLE_ENTITY,
            Problem::Access {
                op: Operation::Write,
                message: "this access policy explanation is intentionally too long for the response buffer",
            },
        );
        let mut response = [0; 64];
        let response = err.response(&mut response);
        assert_eq!(
            response.content_format,
            Some(content_format("application/json"))
        );
        assert!(
            response
                .payload
                .starts_with(br#"{"kind":"access","op":"write","message":""#)
        );
        assert!(response.payload.ends_with(b"\"}"));
    }

    #[test]
    fn coap_message_roundtrip_and_user_route_composition() {
        init_host_logging();
        let request = TestMessage::new(code::PUT)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::CONTENT_FORMAT, content_format("application/json"))
            .with_payload(b"18");
        let request = RequestParts::from_message(&request).unwrap();
        let mut settings = Settings::default();
        let mut response_buf = [0; 128];
        let outcome = route(&request, &mut settings, &mut response_buf);
        assert!(matches!(outcome, Outcome::Changed { .. }));
        let mut response = TestMessage::default();
        outcome.response().unwrap().write_to(&mut response).unwrap();
        assert_eq!(response.code(), code::CHANGED);
        assert_eq!(settings.number, 18);

        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "status")
            .uint_option(option::ACCEPT, content_format("application/json"));
        let request = RequestParts::from_message(&request).unwrap();
        let mut response_buf = [0; 128];
        let outcome = route(&request, &mut settings, &mut response_buf);
        let mut response = TestMessage::default();
        outcome.response().unwrap().write_to(&mut response).unwrap();
        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(ReadableMessage::payload(&response), br#"{"ok":true}"#);
        let content_format_option = response
            .options()
            .find(|opt| opt.number() == option::CONTENT_FORMAT)
            .and_then(|opt| opt.value_uint::<u16>());
        assert_eq!(
            content_format_option,
            Some(content_format("application/json"))
        );
    }

    #[cfg(feature = "cbor")]
    #[test]
    fn const_path_cbor_get_and_put_leaf() {
        init_host_logging();
        let handler = ConstPathCborHandler::const_path_cbor("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let req = request(code::GET, &["settings", "number"], None, b"");
        let out = handler.handle(&req, &mut settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(
            response.content_format,
            Some(content_format("application/cbor"))
        );
        assert_eq!(response.payload, &[7]);

        let mut response = [0; 128];
        let req = request(
            code::PUT,
            &["settings", "number"],
            Some(content_format("application/cbor")),
            &[21],
        );
        let out = handler.handle(&req, &mut settings, &mut response);
        assert!(matches!(out, Outcome::Changed { .. }));
        assert_eq!(settings.number, 21);
    }
}
