use std::net::UdpSocket;

use coap_handler::Handler as _;
use coap_handler_implementations::{HandlerBuilder as _, ReportingHandlerBuilder as _};
use coap_message::error::RenderableOnMinimal as _;
use coap_message_implementations::{inmemory, inmemory_write};
use coap_numbers::code;
use defmt::{info, warn};
use miniconf::Tree;
use miniconf_coap::{ConstPathJsonCoapHandler, JSON_CONTENT_FORMAT, MiniconfSchemaHandler};

const RESPONSE_CAPACITY: usize = 1280;

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

fn main() -> std::io::Result<()> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
    defmt2log::init_from_current_exe();

    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:5683".into());
    let socket = UdpSocket::bind(&bind)?;
    info!("listening on coap://{=str}", bind.as_str());
    info!(
        "try: aiocoap-client coap://{=str}/.well-known/core",
        bind.as_str()
    );
    info!(
        "try: aiocoap-client coap://{=str}/settings/number",
        bind.as_str()
    );
    info!(
        "try: aiocoap-client -m PUT --content-format application/json --payload 12 coap://{=str}/settings/number",
        bind.as_str()
    );

    let mut settings = Settings::default();
    let mut request = [0; RESPONSE_CAPACITY];
    let mut response = [0; RESPONSE_CAPACITY];

    loop {
        let (len, peer) = socket.recv_from(&mut request)?;
        match handle_packet(&request[..len], &mut settings, &mut response) {
            Ok(response_len) => {
                socket.send_to(&response[..response_len], peer)?;
            }
            Err(error) => {
                let peer = peer.to_string();
                warn!("dropping request from {=str}: {=str}", peer.as_str(), error);
            }
        }
    }
}

fn handle_packet(
    request: &[u8],
    settings: &mut Settings,
    response: &mut [u8],
) -> Result<usize, &'static str> {
    let request = WirePacket::parse(request)?;
    let miniconf = ConstPathJsonCoapHandler::const_path_json(settings);
    let schema = MiniconfSchemaHandler::<Settings>::json();
    let mut handler = coap_handler_implementations::new_dispatcher()
        .below(&["settings"], miniconf)
        .at(&["schema"], schema)
        .at(
            &["status"],
            coap_handler_implementations::SimpleRendered::new_typed_str(
                r#"{"ok":true}"#,
                Some(JSON_CONTENT_FORMAT),
            ),
        )
        .with_wkc();

    let mut code = 0;
    let mut tail = [0; RESPONSE_CAPACITY];
    let mut message = inmemory_write::Message::new(&mut code, &mut tail);

    match handler.extract_request_data(&request.message) {
        Ok(data) => handler
            .build_response(&mut message, data)
            .map_err(|_| "response did not fit")?,
        Err(error) => error
            .render(&mut message)
            .map_err(|_| "error response did not fit")?,
    }

    let tail_len = message.finish();
    write_ack(
        response,
        code,
        request.message_id,
        request.token,
        &tail[..tail_len],
    )
}

fn write_ack(
    response: &mut [u8],
    code: u8,
    message_id: [u8; 2],
    token: &[u8],
    tail: &[u8],
) -> Result<usize, &'static str> {
    let len = 4 + token.len() + tail.len();
    if response.len() < len || token.len() > 8 {
        return Err("response buffer too small");
    }
    response[0] = 0x60 | token.len() as u8;
    response[1] = code;
    response[2..4].copy_from_slice(&message_id);
    response[4..4 + token.len()].copy_from_slice(token);
    response[4 + token.len()..len].copy_from_slice(tail);
    Ok(len)
}

#[derive(Debug)]
struct WirePacket<'a> {
    message_id: [u8; 2],
    token: &'a [u8],
    message: inmemory::Message<'a>,
}

impl<'a> WirePacket<'a> {
    fn parse(packet: &'a [u8]) -> Result<Self, &'static str> {
        let [header, request_code, mid_hi, mid_lo, rest @ ..] = packet else {
            return Err("short header");
        };
        if header >> 6 != 1 {
            return Err("unsupported CoAP version");
        }
        if header & 0x30 == 0x30 {
            return Err("reset messages do not get ACK responses");
        }
        let token_len = usize::from(header & 0x0f);
        if token_len > 8 {
            return Err("token too long");
        }
        let Some((token, tail)) = rest.split_at_checked(token_len) else {
            return Err("short token");
        };
        if *request_code == code::EMPTY {
            return Err("empty request");
        }
        Ok(Self {
            message_id: [*mid_hi, *mid_lo],
            token,
            message: inmemory::Message::new(*request_code, tail),
        })
    }
}
