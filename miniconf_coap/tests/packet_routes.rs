use coap_message::ReadableMessage;
use coap_message_implementations::{inmemory, inmemory_write};
use coap_numbers::code;
use miniconf::Tree;
#[cfg(feature = "postcard")]
use miniconf::{Packed, TreeSchema};
#[cfg(feature = "postcard")]
use miniconf_coap::PackedPostcardHandler;
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

    let get_number = [
        0x40,
        0x01,
        0x12,
        0x34, // CON GET mid=0x1234
        0xb8,
        b's',
        b'e',
        b't',
        b't',
        b'i',
        b'n',
        b'g',
        b's', // Uri-Path: settings
        0x06,
        b'n',
        b'u',
        b'm',
        b'b',
        b'e',
        b'r', // Uri-Path: number
        0x61,
        JSON_CONTENT_FORMAT as u8, // Accept: application/json
    ];
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

    let put_label = [
        0x40,
        0x03,
        0x12,
        0x35, // CON PUT mid=0x1235
        0xb8,
        b's',
        b'e',
        b't',
        b't',
        b'i',
        b'n',
        b'g',
        b's', // Uri-Path: settings
        0x05,
        b'l',
        b'a',
        b'b',
        b'e',
        b'l', // Uri-Path: label
        0x11,
        JSON_CONTENT_FORMAT as u8, // Content-Format: application/json
        0xff,
        b'"',
        b'g',
        b'o',
        b'o',
        b'd',
        b'"',
    ];
    let response = handle_packet(&put_label, &mut settings, RouteMode::Json);
    assert_eq!(response, [0x60, 0x44, 0x12, 0x35]); // ACK 2.04 Changed
    assert_eq!(settings.label.as_str(), "good");

    settings.visible = None;
    let absent = [
        0x40, 0x01, 0x12, 0x36, // CON GET mid=0x1236
        0xb8, b's', b'e', b't', b't', b'i', b'n', b'g', b's', // Uri-Path: settings
        0x07, b'v', b'i', b's', b'i', b'b', b'l', b'e', // Uri-Path: visible
        0x05, b'v', b'a', b'l', b'u', b'e', // Uri-Path: value
    ];
    let response = handle_packet(&absent, &mut settings, RouteMode::Json);
    let response = WirePacket::parse(&response).unwrap();
    assert_eq!(response.message.code(), code::CONFLICT);
    assert_eq!(
        response.message.payload(),
        br#"{"kind":"absent","depth":2}"#
    );

    let schema = [
        0x40, 0x01, 0x12, 0x37, // CON GET mid=0x1237
        0xb6, b's', b'c', b'h', b'e', b'm', b'a', // Uri-Path: schema
    ];
    let packet = handle_packet(&schema, &mut settings, RouteMode::Json);
    let response = WirePacket::parse(&packet).unwrap();
    assert_eq!(response.message.code(), code::CONTENT);
    assert!(response.message.payload().starts_with(b"{"));

    let status = [
        0x40, 0x01, 0x12, 0x38, // CON GET mid=0x1238
        0xb6, b's', b't', b'a', b't', b'u', b's', // Uri-Path: status
    ];
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

    let mut get_number = vec![
        0x40, 0x01, 0x12, 0x39, // CON GET mid=0x1239
        0xb8, b's', b'e', b't', b't', b'i', b'n', b'g', b's', // Uri-Path: settings
    ];
    push_uri_path(&mut get_number, key.as_bytes());
    get_number.extend_from_slice(&[
        0x61,
        POSTCARD_CONTENT_FORMAT as u8, // Accept: application/octet-stream
    ]);
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

    let mut put_number = vec![
        0x40, 0x03, 0x12, 0x3a, // CON PUT mid=0x123a
        0xb8, b's', b'e', b't', b't', b'i', b'n', b'g', b's', // Uri-Path: settings
    ];
    push_uri_path(&mut put_number, key.as_bytes());
    put_number.extend_from_slice(&[
        0x11,
        POSTCARD_CONTENT_FORMAT as u8, // Content-Format
        0xff,
        21, // postcard u32 varint
    ]);
    let response = handle_packet(&put_number, &mut settings, RouteMode::Postcard);
    assert_eq!(response, [0x60, 0x44, 0x12, 0x3a]); // ACK 2.04 Changed
    assert_eq!(settings.number, 21);
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

    let mut packet = Vec::new();
    packet.push(0x60 | request.token.len() as u8); // ACK with matching token length
    packet.push(code);
    packet.extend_from_slice(&request.message_id);
    packet.extend_from_slice(request.token);
    packet.extend_from_slice(&tail[..tail_len]);
    packet
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
fn push_uri_path(packet: &mut Vec<u8>, segment: &[u8]) {
    assert!(segment.len() < 13);
    packet.push(segment.len() as u8);
    packet.extend_from_slice(segment);
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
