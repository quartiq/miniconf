#![cfg(feature = "json")]

#[cfg(feature = "coap-handler")]
use coap_handler::Handler as _;
#[cfg(feature = "coap-handler")]
use coap_message::MessageOption;
#[cfg(feature = "coap-handler")]
use coap_message::MinimalWritableMessage as _;
use coap_message::ReadableMessage;
#[cfg(feature = "coap-handler")]
use coap_message::error::RenderableOnMinimal as _;
#[cfg(feature = "coap-handler")]
use coap_message_implementations::heap::HeapMessage;
use coap_message_implementations::{inmemory, inmemory_write};
use coap_numbers::code;
use miniconf::Tree;
#[cfg(feature = "postcard")]
use miniconf::{Packed, TreeSchema};
#[cfg(feature = "postcard")]
use miniconf_coap::PackedPostcardHandler;
#[cfg(feature = "coap-handler")]
use miniconf_coap::{
    ConstPathJson, ConstPathJsonCoapHandler, LINK_FORMAT_CONTENT_FORMAT, MiniconfHandler,
    MiniconfSchemaHandler,
};
use miniconf_coap::{
    ConstPathJsonHandler, JSON_CONTENT_FORMAT, Outcome, RequestParts, Response, SchemaHandler,
};

#[cfg(feature = "postcard")]
const POSTCARD_CONTENT_FORMAT: u16 = 42;

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

#[test]
fn const_path_json_packet_routes_compose_with_user_routes() {
    init_host_logging();
    let mut settings = Settings::default();

    let get_number = Packet::get(0x1234)
        .uri_path("settings")
        .uri_path("number")
        .accept(JSON_CONTENT_FORMAT);
    let response = handle_packet(&get_number, &mut settings, RouteMode::Json);
    assert_eq!(
        response,
        [
            0x60,
            0x45,
            0x12,
            0x34, // ACK 2.05 Content
            0xc1,
            JSON_CONTENT_FORMAT as u8, // Content-Format: application/json
            0xff,
            b'7',
        ]
    );

    let put_label = Packet::put(0x1235)
        .uri_path("settings")
        .uri_path("label")
        .content_format(JSON_CONTENT_FORMAT)
        .payload(br#""good""#);
    let response = handle_packet(&put_label, &mut settings, RouteMode::Json);
    assert_eq!(response, [0x60, 0x44, 0x12, 0x35]); // ACK 2.04 Changed
    assert_eq!(settings.label.as_str(), "good");

    settings.visible = None;
    let absent = Packet::get(0x1236)
        .uri_path("settings")
        .uri_path("visible")
        .uri_path("value");
    let response = handle_packet(&absent, &mut settings, RouteMode::Json);
    let response = WirePacket::parse(&response).unwrap();
    assert_eq!(response.message.code(), code::CONFLICT);
    assert_eq!(
        response.message.payload(),
        br#"{"kind":"absent","depth":2}"#
    );

    let schema = Packet::get(0x1237).uri_path("schema");
    let packet = handle_packet(&schema, &mut settings, RouteMode::Json);
    let response = WirePacket::parse(&packet).unwrap();
    assert_eq!(response.message.code(), code::CONTENT);
    assert!(response.message.payload().starts_with(b"{"));

    let status = Packet::get(0x1238).uri_path("status");
    let packet = handle_packet(&status, &mut settings, RouteMode::Json);
    let response = WirePacket::parse(&packet).unwrap();
    assert_eq!(response.message.code(), code::CONTENT);
    assert_eq!(response.message.payload(), br#"{"ok":true}"#);
}

#[cfg(feature = "postcard")]
#[test]
fn packed_postcard_packet_route_reads_and_writes() {
    init_host_logging();
    let mut settings = Settings::default();
    let number_key = number_packed_key();
    let key = number_key.to_string();

    let get_number = Packet::get(0x1239)
        .uri_path("settings")
        .uri_path(&key)
        .accept(POSTCARD_CONTENT_FORMAT);
    let response = handle_packet(&get_number, &mut settings, RouteMode::Postcard);
    assert_eq!(
        response,
        [
            0x60,
            0x45,
            0x12,
            0x39, // ACK 2.05 Content
            0xc1,
            POSTCARD_CONTENT_FORMAT as u8, // Content-Format
            0xff,
            7, // postcard u32 varint
        ]
    );

    let put_number = Packet::put(0x123a)
        .uri_path("settings")
        .uri_path(&key)
        .content_format(POSTCARD_CONTENT_FORMAT)
        .payload(&[21]);
    let response = handle_packet(&put_number, &mut settings, RouteMode::Postcard);
    assert_eq!(response, [0x60, 0x44, 0x12, 0x3a]); // ACK 2.04 Changed
    assert_eq!(settings.number, 21);
}

#[cfg(feature = "coap-handler")]
#[test]
fn coap_handler_reports_and_serves_well_known_core() {
    init_host_logging();

    let mut settings = Settings::default();
    let mut handler = demo_handler(&mut settings);

    let request = heap_request(
        code::GET,
        &[".well-known", "core"],
        Some(LINK_FORMAT_CONTENT_FORMAT),
        b"",
    );
    let response = handle_heap(&mut handler, &request);

    assert_eq!(response.code(), code::CONTENT);
    assert_eq!(
        first_uint_option(&response, coap_numbers::option::CONTENT_FORMAT),
        Some(LINK_FORMAT_CONTENT_FORMAT)
    );
    let payload = core::str::from_utf8(response.payload()).unwrap();
    assert!(payload.contains("</settings/number>;ct=50;rt=\"miniconf.leaf\""));
    assert!(payload.contains("</schema>;ct=50;rt=\"miniconf.schema\""));
    assert!(payload.contains("</status>"));
}

#[cfg(feature = "coap-handler")]
#[test]
fn coap_handler_serves_values_beside_user_routes() {
    init_host_logging();

    let mut settings = Settings::default();
    let mut handler = demo_handler(&mut settings);

    let request = heap_request(
        code::GET,
        &["settings", "number"],
        Some(JSON_CONTENT_FORMAT),
        b"",
    );
    let response = handle_heap(&mut handler, &request);
    assert_eq!(response.code(), code::CONTENT);
    assert_eq!(response.payload(), b"7");

    let request = heap_request(code::GET, &["status"], Some(JSON_CONTENT_FORMAT), b"");
    let response = handle_heap(&mut handler, &request);
    assert_eq!(response.code(), code::CONTENT);
    assert_eq!(response.payload(), br#"{"ok":true}"#);
}

#[cfg(feature = "coap-handler")]
#[test]
fn coap_handler_can_own_settings() {
    init_host_logging();
    use coap_handler_implementations::HandlerBuilder as _;

    let miniconf =
        MiniconfHandler::<Settings, Settings, ConstPathJson>::const_path_json(Settings::default());
    let mut handler = coap_handler_implementations::new_dispatcher().below(&["settings"], miniconf);

    let mut request = heap_request(code::PUT, &["settings", "number"], None, br#"31"#);
    request
        .add_option_uint(coap_numbers::option::CONTENT_FORMAT, JSON_CONTENT_FORMAT)
        .unwrap();
    let response = handle_heap(&mut handler, &request);
    assert_eq!(response.code(), code::CHANGED);

    let request = heap_request(
        code::GET,
        &["settings", "number"],
        Some(JSON_CONTENT_FORMAT),
        b"",
    );
    let response = handle_heap(&mut handler, &request);
    assert_eq!(response.code(), code::CONTENT);
    assert_eq!(response.payload(), b"31");
}

#[cfg(feature = "coap-handler")]
#[test]
fn coap_handler_routes_binary_packets() {
    init_host_logging();
    let mut settings = Settings::default();

    let get_number = Packet::get(0x1234).uri_path("settings").uri_path("number");
    let response = handle_packet_with_coap_handler(&get_number, &mut settings);
    assert_eq!(&response[..4], [0x60, code::CONTENT, 0x12, 0x34]);
    assert_eq!(
        &response[4..],
        [0xc1, JSON_CONTENT_FORMAT as u8, 0xff, b'7']
    );

    let put_number = Packet::put(0x1235)
        .uri_path("settings")
        .uri_path("number")
        .content_format(JSON_CONTENT_FORMAT)
        .payload(b"23");
    let response = handle_packet_with_coap_handler(&put_number, &mut settings);
    assert_eq!(response, [0x60, code::CHANGED, 0x12, 0x35]);
    assert_eq!(settings.number, 23);
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
        self.option(coap_numbers::option::URI_PATH, segment.as_bytes())
    }

    fn accept(self, content_format: u16) -> Self {
        self.uint_option(coap_numbers::option::ACCEPT, content_format)
    }

    fn content_format(self, content_format: u16) -> Self {
        self.uint_option(coap_numbers::option::CONTENT_FORMAT, content_format)
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

#[derive(Clone, Copy)]
enum RouteMode {
    Json,
    #[cfg(feature = "postcard")]
    Postcard,
}

fn handle_packet(packet: &[u8], settings: &mut Settings, mode: RouteMode) -> Vec<u8> {
    let request = WirePacket::parse(packet).unwrap();
    let parts = RequestParts::from_message(&request.message).unwrap();
    let mut response_buf = [0; 512];
    let outcome = route(&parts, settings, &mut response_buf, mode);
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

#[cfg(feature = "coap-handler")]
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

#[cfg(feature = "coap-handler")]
fn demo_handler(
    settings: &mut Settings,
) -> impl coap_handler::Handler + coap_handler::Reporting + '_ {
    use coap_handler_implementations::{HandlerBuilder as _, ReportingHandlerBuilder as _};

    coap_handler_implementations::new_dispatcher()
        .below(
            &["settings"],
            ConstPathJsonCoapHandler::const_path_json(settings),
        )
        .at(&["schema"], MiniconfSchemaHandler::<Settings>::json())
        .at(
            &["status"],
            coap_handler_implementations::SimpleRendered::new_typed_str(
                r#"{"ok":true}"#,
                Some(JSON_CONTENT_FORMAT),
            ),
        )
        .with_wkc()
}

fn route<'a>(
    request: &RequestParts<'_>,
    settings: &mut Settings,
    response_buf: &'a mut [u8],
    mode: RouteMode,
) -> Outcome<'a> {
    match request.path() {
        "/schema" => SchemaHandler::new("/schema").handle::<Settings>(request, response_buf),
        "/settings" => settings_route(request, settings, response_buf, mode),
        path if path.starts_with("/settings/") => {
            settings_route(request, settings, response_buf, mode)
        }
        "/status" => Outcome::Handled(Response {
            code: code::CONTENT,
            content_format: Some(JSON_CONTENT_FORMAT),
            payload: br#"{"ok":true}"#,
        }),
        _ => Outcome::Unhandled,
    }
}

fn encode_ack(code: u8, message_id: [u8; 2], token: &[u8], tail: &[u8]) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.push(0x60 | token.len() as u8); // ACK with matching token length
    packet.push(code);
    packet.extend_from_slice(&message_id);
    packet.extend_from_slice(token);
    packet.extend_from_slice(tail);
    packet
}

fn settings_route<'a>(
    request: &RequestParts<'_>,
    settings: &mut Settings,
    response_buf: &'a mut [u8],
    mode: RouteMode,
) -> Outcome<'a> {
    match mode {
        RouteMode::Json => ConstPathJsonHandler::const_path_json("/settings").handle(
            request,
            settings,
            response_buf,
        ),
        #[cfg(feature = "postcard")]
        RouteMode::Postcard => PackedPostcardHandler::packed_postcard(
            "/settings",
            POSTCARD_CONTENT_FORMAT,
        )
        .handle(request, settings, response_buf),
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

#[cfg(feature = "postcard")]
fn number_packed_key() -> usize {
    Settings::SCHEMA
        .nodes::<Packed, { miniconf_coap::MAX_DEPTH }>()
        .nth(1)
        .unwrap()
        .unwrap()
        .into_lsb()
        .get()
}

fn init_host_logging() {
    static HOST_LOGGING: std::sync::OnceLock<()> = std::sync::OnceLock::new();

    HOST_LOGGING.get_or_init(|| {
        let _ = env_logger::builder().is_test(true).try_init();
        defmt2log::init_from_current_exe();
    });
}

#[cfg(feature = "coap-handler")]
fn heap_request(code: u8, path: &[&str], accept: Option<u16>, payload: &[u8]) -> HeapMessage {
    let mut request = HeapMessage::new();
    request.set_code(code);
    for segment in path {
        request
            .add_option_str(coap_numbers::option::URI_PATH, segment)
            .unwrap();
    }
    if let Some(accept) = accept {
        request
            .add_option_uint(coap_numbers::option::ACCEPT, accept)
            .unwrap();
    }
    request.set_payload(payload).unwrap();
    request
}

#[cfg(feature = "coap-handler")]
fn handle_heap<H>(handler: &mut H, request: &HeapMessage) -> HeapMessage
where
    H: coap_handler::Handler,
    H::ExtractRequestError: core::fmt::Debug,
    H::BuildResponseError<HeapMessage>: core::fmt::Debug,
{
    let data = handler.extract_request_data(request).unwrap();
    let mut response = HeapMessage::new();
    handler.build_response(&mut response, data).unwrap();
    response
}

#[cfg(feature = "coap-handler")]
fn first_uint_option(message: &HeapMessage, number: u16) -> Option<u16> {
    message
        .options()
        .find(|option| option.number() == number)
        .and_then(|option| option.value_uint())
}
