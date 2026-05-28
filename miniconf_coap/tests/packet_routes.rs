#![cfg(any(feature = "json-core", feature = "cbor"))]

use coap_message_implementations::{inmemory, inmemory_write};
use coap_numbers::{code, option};
use miniconf::Tree;
use miniconf_coap::{Outcome, RequestParts, Response};

#[derive(Tree)]
struct Settings {
    hidden: bool,
    #[tree(meta(title = "Demo number"))]
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

#[cfg(feature = "json-core")]
mod json {
    use coap_message::ReadableMessage as _;
    use coap_numbers::{code, content_format};
    use miniconf_coap::{ConstPathJsonHandler, Outcome, SchemaHandler};

    use crate::{
        Packet, RequestParts, Response, Settings, WirePacket, init_host_logging, response_packet,
    };

    #[test]
    fn const_path_json_packet_routes_compose_with_user_routes() {
        init_host_logging();
        let mut settings = Settings::default();

        let get_number = Packet::get(0x1234)
            .uri_path("settings")
            .uri_path("number")
            .accept(content_format::from_str("application/json").unwrap());
        let response = handle_json_packet(&get_number, &mut settings);
        assert_eq!(
            response,
            [
                0x60,
                0x45,
                0x12,
                0x34,
                0xc1,
                content_format::from_str("application/json").unwrap() as u8,
                0xff,
                b'7',
            ]
        );

        let put_label = Packet::put(0x1235)
            .uri_path("settings")
            .uri_path("label")
            .content_format(content_format::from_str("application/json").unwrap())
            .payload(br#""good""#);
        let response = handle_json_packet(&put_label, &mut settings);
        assert_eq!(response, [0x60, 0x44, 0x12, 0x35]);
        assert_eq!(settings.label.as_str(), "good");

        settings.visible = None;
        let absent = Packet::get(0x1236)
            .uri_path("settings")
            .uri_path("visible")
            .uri_path("value");
        let response = handle_json_packet(&absent, &mut settings);
        let response = WirePacket::parse(&response).unwrap();
        assert_eq!(response.message.code(), code::CONFLICT);
        assert_eq!(
            response.message.payload(),
            br#"{"kind":"absent","depth":2}"#
        );

        let schema = Packet::get(0x1237).uri_path("schema");
        let packet = handle_json_packet(&schema, &mut settings);
        let response = WirePacket::parse(&packet).unwrap();
        assert_eq!(response.message.code(), code::CONTENT);
        assert!(
            response
                .message
                .payload()
                .starts_with(br#"{"proto":1,"epoch":0,"schema_rev":"#)
        );

        let schema_page = Packet::get(0x1238).uri_path("schema").uri_path("0");
        let packet = handle_json_packet(&schema_page, &mut settings);
        let response = WirePacket::parse(&packet).unwrap();
        assert_eq!(response.message.code(), code::CONTENT);
        assert!(response.message.payload().starts_with(b"{"));

        let status = Packet::get(0x1239).uri_path("status");
        let packet = handle_json_packet(&status, &mut settings);
        let response = WirePacket::parse(&packet).unwrap();
        assert_eq!(response.message.code(), code::CONTENT);
        assert_eq!(response.message.payload(), br#"{"ok":true}"#);
    }

    pub fn handle_json_packet(packet: &[u8], settings: &mut Settings) -> Vec<u8> {
        let request = WirePacket::parse(packet).unwrap();
        let parts = RequestParts::from_message(&request.message).unwrap();
        let mut response_buf = [0; 512];
        let outcome = route(&parts, settings, &mut response_buf);
        response_packet(&request, outcome)
    }

    fn route<'a>(
        request: &RequestParts<'_>,
        settings: &mut Settings,
        response_buf: &'a mut [u8],
    ) -> Outcome<'a> {
        match request.path() {
            "/schema" => SchemaHandler::new("/schema").handle::<Settings>(request, response_buf),
            path if path.starts_with("/schema/") => {
                SchemaHandler::new("/schema").handle::<Settings>(request, response_buf)
            }
            "/settings" => settings_route(request, settings, response_buf),
            path if path.starts_with("/settings/") => {
                settings_route(request, settings, response_buf)
            }
            "/status" => Outcome::Handled(Response {
                code: code::CONTENT,
                content_format: Some(content_format::from_str("application/json").unwrap()),
                payload: br#"{"ok":true}"#,
            }),
            _ => Outcome::Unhandled,
        }
    }

    fn settings_route<'a>(
        request: &RequestParts<'_>,
        settings: &mut Settings,
        response_buf: &'a mut [u8],
    ) -> Outcome<'a> {
        ConstPathJsonHandler::const_path_json("/settings").handle(request, settings, response_buf)
    }
}

#[cfg(feature = "cbor")]
mod cbor {
    use coap_message::{MessageOption as _, ReadableMessage as _};
    use coap_numbers::{code, content_format, option};
    use minicbor::{Decoder, data::Type};
    use miniconf_coap::{ConstPathCborHandler, Outcome};

    use crate::{Packet, RequestParts, Settings, WirePacket, init_host_logging, response_packet};

    #[test]
    fn const_path_cbor_packet_route_reads_and_writes() {
        init_host_logging();
        let mut settings = Settings::default();

        let get_number = Packet::get(0x1239)
            .uri_path("settings")
            .uri_path("number")
            .accept(content_format::from_str("application/cbor").unwrap());
        let response = handle_cbor_packet(&get_number, &mut settings);
        assert_eq!(
            response,
            [
                0x60,
                0x45,
                0x12,
                0x39,
                0xc1,
                content_format::from_str("application/cbor").unwrap() as u8,
                0xff,
                7,
            ]
        );

        let put_number = Packet::put(0x123a)
            .uri_path("settings")
            .uri_path("number")
            .content_format(content_format::from_str("application/cbor").unwrap())
            .payload(&[21]);
        let response = handle_cbor_packet(&put_number, &mut settings);
        assert_eq!(response, [0x60, 0x44, 0x12, 0x3a]);
        assert_eq!(settings.number, 21);

        let trailing = Packet::put(0x123b)
            .uri_path("settings")
            .uri_path("number")
            .content_format(content_format::from_str("application/cbor").unwrap())
            .payload(&[22, 0]);
        let response = handle_cbor_packet(&trailing, &mut settings);
        let response = WirePacket::parse(&response).unwrap();
        assert_eq!(response.message.code(), code::BAD_REQUEST);
        assert_eq!(
            response
                .message
                .options()
                .find(|option| option.number() == option::CONTENT_FORMAT)
                .and_then(|option| option.value_uint::<u16>()),
            Some(content_format::from_str("application/concise-problem-details+cbor").unwrap())
        );
        assert_cbor_problem(response.message.payload(), code::BAD_REQUEST, "bad_payload");
        assert_eq!(settings.number, 21);
    }

    fn handle_cbor_packet(packet: &[u8], settings: &mut Settings) -> Vec<u8> {
        let request = WirePacket::parse(packet).unwrap();
        let parts = RequestParts::from_message(&request.message).unwrap();
        let mut response_buf = [0; 512];
        let outcome = match parts.path() {
            "/settings" => settings_route(&parts, settings, &mut response_buf),
            path if path.starts_with("/settings/") => {
                settings_route(&parts, settings, &mut response_buf)
            }
            _ => Outcome::Unhandled,
        };
        response_packet(&request, outcome)
    }

    fn settings_route<'a>(
        request: &RequestParts<'_>,
        settings: &mut Settings,
        response_buf: &'a mut [u8],
    ) -> Outcome<'a> {
        ConstPathCborHandler::const_path_cbor("/settings").handle(request, settings, response_buf)
    }

    fn assert_cbor_problem(payload: &[u8], response_code: u8, kind: &str) {
        let mut decoder = Decoder::new(payload);
        assert_eq!(decoder.map().unwrap(), Some(3));
        let mut saw_title = false;
        let mut saw_response_code = false;
        let mut saw_miniconf = false;

        for _ in 0..3 {
            match decoder.datatype().unwrap() {
                Type::I8
                | Type::I16
                | Type::I32
                | Type::I64
                | Type::Int
                | Type::U8
                | Type::U16
                | Type::U32
                | Type::U64 => match i128::from(decoder.int().unwrap()) {
                    -1 => {
                        assert_eq!(decoder.str().unwrap(), kind);
                        saw_title = true;
                    }
                    -4 => {
                        assert_eq!(decoder.u8().unwrap(), response_code);
                        saw_response_code = true;
                    }
                    _ => decoder.skip().unwrap(),
                },
                Type::String => {
                    let key = decoder.str().unwrap();
                    if key == "tag:quartiq.de,2026:miniconf" {
                        assert_eq!(decoder.map().unwrap(), Some(1));
                        assert_eq!(decoder.str().unwrap(), "kind");
                        assert_eq!(decoder.str().unwrap(), kind);
                        saw_miniconf = true;
                    } else {
                        decoder.skip().unwrap();
                    }
                }
                ty => panic!("unexpected problem key type {ty:?}"),
            }
        }

        assert!(saw_title);
        assert!(saw_response_code);
        assert!(saw_miniconf);
        assert_eq!(decoder.position(), payload.len());
    }
}

#[cfg(all(feature = "coap-handler", feature = "json-core"))]
mod coap_handler {
    use core::fmt;

    use coap_handler::Handler as _;
    use coap_handler_implementations::{
        HandlerBuilder as _, ReportingHandlerBuilder as _, SimpleRendered, new_dispatcher,
    };
    use coap_message::{
        MessageOption, MinimalWritableMessage as _, ReadableMessage as _,
        error::RenderableOnMinimal as _,
    };
    use coap_message_implementations::{heap::HeapMessage, inmemory_write};
    use coap_numbers::{code, content_format, option};
    use miniconf_coap::{
        ConstPathJson, ConstPathJsonCoapHandler, MiniconfHandler, MiniconfSchemaHandler,
    };

    use crate::{Packet, Settings, WirePacket, encode_ack, init_host_logging};

    #[test]
    fn reports_and_serves_well_known_core() {
        init_host_logging();

        let mut settings = Settings::default();
        let mut handler = demo_handler(&mut settings);

        let request = heap_request(
            code::GET,
            &[".well-known", "core"],
            Some(content_format::from_str("application/link-format").unwrap()),
            b"",
        );
        let response = handle_heap(&mut handler, &request);

        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(
            first_uint_option(&response, option::CONTENT_FORMAT),
            Some(content_format::from_str("application/link-format").unwrap())
        );
        let payload = core::str::from_utf8(response.payload()).unwrap();
        assert!(payload.contains("</settings/number>;ct=50;title=\"Demo number\""));
        assert!(!payload.contains("rt=\"miniconf.leaf\""));
        assert!(payload.contains("</schema>;ct=50;rt=\"miniconf.schema\""));
        assert!(payload.contains("</status>"));
    }

    #[test]
    fn serves_values_beside_user_routes() {
        init_host_logging();

        let mut settings = Settings::default();
        let mut handler = demo_handler(&mut settings);

        let request = heap_request(
            code::GET,
            &["settings", "number"],
            Some(content_format::from_str("application/json").unwrap()),
            b"",
        );
        let response = handle_heap(&mut handler, &request);
        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(response.payload(), b"7");

        let request = heap_request(
            code::GET,
            &["status"],
            Some(content_format::from_str("application/json").unwrap()),
            b"",
        );
        let response = handle_heap(&mut handler, &request);
        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(response.payload(), br#"{"ok":true}"#);
    }

    #[test]
    fn can_own_settings() {
        init_host_logging();
        let miniconf = MiniconfHandler::<Settings, Settings, ConstPathJson>::const_path_json(
            Settings::default(),
        );
        let mut handler = new_dispatcher().below(&["settings"], miniconf);

        let mut request = heap_request(code::PUT, &["settings", "number"], None, br#"31"#);
        request
            .add_option_uint(
                option::CONTENT_FORMAT,
                content_format::from_str("application/json").unwrap(),
            )
            .unwrap();
        let response = handle_heap(&mut handler, &request);
        assert_eq!(response.code(), code::CHANGED);

        let request = heap_request(
            code::GET,
            &["settings", "number"],
            Some(content_format::from_str("application/json").unwrap()),
            b"",
        );
        let response = handle_heap(&mut handler, &request);
        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(response.payload(), b"31");
    }

    #[test]
    fn routes_binary_packets() {
        init_host_logging();
        let mut settings = Settings::default();

        let get_number = Packet::get(0x1234).uri_path("settings").uri_path("number");
        let response = handle_packet_with_coap_handler(&get_number, &mut settings);
        assert_eq!(&response[..4], [0x60, code::CONTENT, 0x12, 0x34]);
        assert_eq!(
            &response[4..],
            [
                0xc1,
                content_format::from_str("application/json").unwrap() as u8,
                0xff,
                b'7'
            ]
        );

        let schema_manifest = Packet::get(0x1235).uri_path("schema");
        let response = handle_packet_with_coap_handler(&schema_manifest, &mut settings);
        let response = WirePacket::parse(&response).unwrap();
        assert_eq!(response.message.code(), code::CONTENT);
        assert!(
            response
                .message
                .payload()
                .starts_with(br#"{"proto":1,"epoch":0,"schema_rev":"#)
        );

        let schema_page = Packet::get(0x1236).uri_path("schema").uri_path("0");
        let response = handle_packet_with_coap_handler(&schema_page, &mut settings);
        let response = WirePacket::parse(&response).unwrap();
        assert_eq!(response.message.code(), code::CONTENT);
        assert!(response.message.payload().starts_with(b"{"));

        let put_number = Packet::put(0x1237)
            .uri_path("settings")
            .uri_path("number")
            .content_format(content_format::from_str("application/json").unwrap())
            .payload(b"23");
        let response = handle_packet_with_coap_handler(&put_number, &mut settings);
        assert_eq!(response, [0x60, code::CHANGED, 0x12, 0x37]);
        assert_eq!(settings.number, 23);

        let bad_put = Packet::put(0x1238)
            .uri_path("settings")
            .uri_path("number")
            .payload(b"24");
        let response = handle_packet_with_coap_handler(&bad_put, &mut settings);
        let response = WirePacket::parse(&response).unwrap();
        assert_eq!(response.message.code(), code::UNSUPPORTED_CONTENT_FORMAT);
        assert_eq!(
            response.message.payload(),
            br#"{"kind":"unsupported_content_format"}"#
        );
        assert_eq!(settings.number, 23);
    }

    fn handle_packet_with_coap_handler(packet: &[u8], settings: &mut Settings) -> Vec<u8> {
        let request = WirePacket::parse(packet).unwrap();
        let mut handler = demo_handler(settings);

        let mut code = 0;
        let mut tail = [0; 512];
        let mut message = inmemory_write::Message::new(&mut code, &mut tail);

        match handler.extract_request_data(&request.message) {
            Ok(data) => handler.build_response(&mut message, data).unwrap(),
            Err(error) => error.render(&mut message).unwrap(),
        }

        let tail_len = message.finish();
        encode_ack(code, request.message_id, request.token, &tail[..tail_len])
    }

    fn demo_handler(
        settings: &mut Settings,
    ) -> impl coap_handler::Handler + coap_handler::Reporting + '_ {
        new_dispatcher()
            .below(
                &["settings"],
                ConstPathJsonCoapHandler::const_path_json(settings),
            )
            .below(&["schema"], MiniconfSchemaHandler::<Settings>::json())
            .at(
                &["status"],
                SimpleRendered::new_typed_str(
                    r#"{"ok":true}"#,
                    Some(content_format::from_str("application/json").unwrap()),
                ),
            )
            .with_wkc()
    }

    fn heap_request(code: u8, path: &[&str], accept: Option<u16>, payload: &[u8]) -> HeapMessage {
        let mut request = HeapMessage::new();
        request.set_code(code);
        for segment in path {
            request.add_option_str(option::URI_PATH, segment).unwrap();
        }
        if let Some(accept) = accept {
            request.add_option_uint(option::ACCEPT, accept).unwrap();
        }
        request.set_payload(payload).unwrap();
        request
    }

    fn handle_heap<H>(handler: &mut H, request: &HeapMessage) -> HeapMessage
    where
        H: coap_handler::Handler,
        H::ExtractRequestError: fmt::Debug,
        H::BuildResponseError<HeapMessage>: fmt::Debug,
    {
        let data = handler.extract_request_data(request).unwrap();
        let mut response = HeapMessage::new();
        handler.build_response(&mut response, data).unwrap();
        response
    }

    fn first_uint_option(message: &HeapMessage, number: u16) -> Option<u16> {
        message
            .options()
            .find(|option| option.number() == number)
            .and_then(|option| option.value_uint())
    }
}

struct Packet {
    bytes: Vec<u8>,
    last_option: u16,
}

impl Packet {
    fn get(message_id: u16) -> Self {
        Self::new(code::GET, message_id)
    }

    fn put(message_id: u16) -> Self {
        Self::new(code::PUT, message_id)
    }

    fn new(code: u8, message_id: u16) -> Self {
        Self {
            bytes: vec![0x40, code, (message_id >> 8) as u8, message_id as u8],
            last_option: 0,
        }
    }

    fn uri_path(self, segment: &str) -> Self {
        self.option(option::URI_PATH, segment.as_bytes())
    }

    fn accept(self, content_format: u16) -> Self {
        self.uint_option(option::ACCEPT, content_format)
    }

    fn content_format(self, content_format: u16) -> Self {
        self.uint_option(option::CONTENT_FORMAT, content_format)
    }

    fn uint_option(self, number: u16, value: u16) -> Self {
        if value == 0 {
            self.option(number, &[])
        } else if value < 256 {
            self.option(number, &[value as u8])
        } else {
            self.option(number, &value.to_be_bytes())
        }
    }

    fn option(mut self, number: u16, value: &[u8]) -> Self {
        let delta = number.checked_sub(self.last_option).unwrap();
        assert!(delta < 13);
        assert!(value.len() < 13);
        self.bytes.push(((delta as u8) << 4) | value.len() as u8);
        self.bytes.extend_from_slice(value);
        self.last_option = number;
        self
    }

    fn payload(mut self, payload: &[u8]) -> Self {
        if !payload.is_empty() {
            self.bytes.push(0xff);
            self.bytes.extend_from_slice(payload);
        }
        self
    }
}

impl core::ops::Deref for Packet {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

#[derive(Debug)]
struct WirePacket<'a> {
    message_id: [u8; 2],
    token: &'a [u8],
    message: inmemory::Message<'a>,
}

impl<'a> WirePacket<'a> {
    fn parse(packet: &'a [u8]) -> Result<Self, &'static str> {
        let [header, code, mid_hi, mid_lo, rest @ ..] = packet else {
            return Err("short header");
        };
        if header >> 6 != 1 {
            return Err("unsupported CoAP version");
        }
        let token_len = usize::from(header & 0x0f);
        let Some((token, rest)) = rest.split_at_checked(token_len) else {
            return Err("short token");
        };
        Ok(Self {
            message_id: [*mid_hi, *mid_lo],
            token,
            message: inmemory::Message::new(*code, rest),
        })
    }
}

fn response_packet(request: &WirePacket<'_>, outcome: Outcome<'_>) -> Vec<u8> {
    let response = outcome.response().unwrap_or(Response {
        code: code::NOT_FOUND,
        content_format: None,
        payload: b"",
    });
    let mut code = 0;
    let mut tail = [0; 512];
    let mut message = inmemory_write::Message::new(&mut code, &mut tail);
    response.write_to(&mut message).unwrap();
    let tail_len = message.finish();
    encode_ack(code, request.message_id, request.token, &tail[..tail_len])
}

fn encode_ack(code: u8, message_id: [u8; 2], token: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.push(0x60 | token.len() as u8);
    packet.push(code);
    packet.extend_from_slice(&message_id);
    packet.extend_from_slice(token);
    packet.extend_from_slice(tail);
    packet
}

fn init_host_logging() {
    static HOST_LOGGING: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    HOST_LOGGING.get_or_init(|| {
        let _ = env_logger::builder().is_test(true).try_init();
        defmt2log::init_from_current_exe();
    });
}
