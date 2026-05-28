use core::{convert::Infallible, fmt};
#[cfg(feature = "json-core")]
use fmt::Write as _;

#[cfg(any(feature = "json-core", feature = "cbor"))]
use coap_message::MutableWritableMessage;
use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, OptionNumber as _, ReadableMessage,
    error::RenderableOnMinimal,
};
use coap_numbers::{code, option};
#[cfg(feature = "cbor")]
use minicbor::{
    Encoder as CborEncoder,
    data::Int as CborInt,
    encode::{self, write::EndOfSlice},
};

#[cfg(any(feature = "json-core", feature = "cbor"))]
use crate::format;
use crate::{ChangedKey, MAX_URI_PATH_LENGTH};

const MAX_ACCEPT_OPTIONS: usize = 4;

pub(crate) type UriPath = heapless::String<MAX_URI_PATH_LENGTH>;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Accepts {
    values: [u16; MAX_ACCEPT_OPTIONS],
    len: usize,
    overflow: bool,
}

impl Accepts {
    const fn new() -> Self {
        Self {
            values: [0; MAX_ACCEPT_OPTIONS],
            len: 0,
            overflow: false,
        }
    }

    fn from_option(value: Option<u16>) -> Self {
        let mut accepts = Self::new();
        if let Some(value) = value {
            accepts.values[0] = value;
            accepts.len = 1;
        }
        accepts
    }

    fn push(&mut self, value: u16) {
        if let Some(slot) = self.values.get_mut(self.len) {
            *slot = value;
            self.len += 1;
        } else {
            self.overflow = true;
        }
    }

    const fn is_present(&self) -> bool {
        self.len != 0
    }

    fn first(&self) -> Option<u16> {
        self.values.first().copied().filter(|_| self.len != 0)
    }

    fn contains(&self, content_format: u16) -> bool {
        self.values[..self.len].contains(&content_format)
    }

    fn accepts(&self, content_format: u16) -> Result<(), Error> {
        if !self.is_present() || self.contains(content_format) {
            Ok(())
        } else if self.overflow {
            Err(Error::new(code::BAD_OPTION, Problem::TooManyAcceptOptions))
        } else {
            Err(Error::new(code::NOT_ACCEPTABLE, Problem::NotAcceptable))
        }
    }
}

/// Parsed CoAP request data used by cooperative handlers.
#[derive(Debug)]
pub struct RequestParts<'a> {
    pub(crate) code: u8,
    pub(crate) path: UriPath,
    pub(crate) accepts: Accepts,
    pub(crate) content_format: Option<u16>,
    pub(crate) invalid_option: Option<InvalidOption>,
    pub(crate) payload: &'a [u8],
}

impl<'a> RequestParts<'a> {
    /// Build request parts from already-decoded fields.
    pub fn new(
        code: u8,
        path: &[&str],
        accept: Option<u16>,
        content_format: Option<u16>,
        payload: &'a [u8],
    ) -> Result<Self, Error> {
        let mut request = Self {
            code,
            path: UriPath::new(),
            accepts: Accepts::from_option(accept),
            content_format,
            invalid_option: None,
            payload,
        };
        for segment in path {
            request.push_path_segment(segment)?;
        }
        Ok(request)
    }

    /// Extract method, URI path, content negotiation options, and payload from a readable message.
    pub fn from_message<M>(message: &'a M) -> Result<Self, Error>
    where
        M: ReadableMessage + ?Sized,
    {
        let mut request = Self {
            code: message.code().into(),
            path: UriPath::new(),
            accepts: Accepts::new(),
            content_format: None,
            invalid_option: None,
            payload: message.payload(),
        };

        for opt in message.options() {
            match opt.number() {
                option::URI_PATH => {
                    let Some(segment) = opt.value_str() else {
                        return Err(Error::bad_request(Problem::InvalidUriPath));
                    };
                    request.push_path_segment(segment)?;
                }
                option::ACCEPT => {
                    let Some(accept) = opt.value_uint() else {
                        return Err(Error::bad_request(Problem::InvalidAccept));
                    };
                    request.accepts.push(accept);
                }
                option::CONTENT_FORMAT => {
                    let Some(content_format) = opt.value_uint() else {
                        return Err(Error::bad_request(Problem::InvalidContentFormat));
                    };
                    if request.content_format.is_some() {
                        if request.invalid_option.is_none() {
                            request.invalid_option = Some(InvalidOption::DuplicateContentFormat);
                        }
                        continue;
                    }
                    request.content_format = Some(content_format);
                }
                option::URI_HOST | option::URI_PORT => {}
                number
                    if option::get_criticality(number) == option::Criticality::Critical
                        && request.invalid_option.is_none() =>
                {
                    request.invalid_option = Some(InvalidOption::UnknownCritical(number));
                }
                _ => {}
            }
        }

        Ok(request)
    }

    /// Request method code.
    pub const fn code(&self) -> u8 {
        self.code
    }

    /// URI path segments.
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    /// Request payload.
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    pub(crate) fn accepts(&self, content_format: u16) -> Result<(), Error> {
        self.accepts.accepts(content_format)
    }

    pub(crate) fn check_options(&self) -> Result<(), Error> {
        match self.invalid_option {
            Some(InvalidOption::DuplicateContentFormat) => {
                Err(Error::bad_request(Problem::DuplicateContentFormat))
            }
            Some(InvalidOption::UnknownCritical(number)) => Err(Error::new(
                code::BAD_OPTION,
                Problem::UnknownCriticalOption { number },
            )),
            None => Ok(()),
        }
    }

    fn push_path_segment(&mut self, segment: &str) -> Result<(), Error> {
        if segment.contains('/') {
            return Err(Error::bad_request(Problem::InvalidUriPath));
        }
        self.path
            .push('/')
            .map_err(|_| Error::request_entity_too_large(Problem::UriPathTooLong))?;
        self.path
            .push_str(segment)
            .map_err(|_| Error::request_entity_too_large(Problem::UriPathTooLong))
    }

    pub(crate) fn relative_to(&self, base: &str) -> Option<&str> {
        if base.is_empty() {
            return Some(self.path.as_str());
        }
        if self.path.as_str() == base {
            return Some("");
        }
        let tail = self.path.as_str().strip_prefix(base)?;
        tail.starts_with('/').then_some(tail)
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum InvalidOption {
    DuplicateContentFormat,
    UnknownCritical(u16),
}

impl defmt::Format for RequestParts<'_> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        defmt::write!(
            fmt,
            "RequestParts {{ code: {=u8}, path: {=str}, accept_present: {=bool}, accept: {=u16}, content_format_present: {=bool}, content_format: {=u16}, payload_len: {=usize} }}",
            self.code,
            self.path.as_str(),
            self.accepts.is_present(),
            self.accepts.first().unwrap_or(0),
            self.content_format.is_some(),
            self.content_format.unwrap_or(0),
            self.payload.len()
        )
    }
}

/// CoAP response data produced by cooperative handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response<'a> {
    /// CoAP response code.
    pub code: u8,
    /// Optional CoAP Content-Format value.
    pub content_format: Option<u16>,
    /// Response payload.
    pub payload: &'a [u8],
}

impl Response<'_> {
    /// Render this response into a writable CoAP message.
    pub fn write_to<M: MinimalWritableMessage>(
        &self,
        message: &mut M,
    ) -> Result<(), M::UnionError> {
        message.set_code(M::Code::new(self.code).map_err(M::convert_code_error)?);
        if let Some(content_format) = self.content_format {
            message
                .add_option_uint(
                    M::OptionNumber::new(option::CONTENT_FORMAT)
                        .map_err(M::convert_option_number_error)?,
                    content_format,
                )
                .map_err(M::convert_add_option_error)?;
        }
        message
            .set_payload(self.payload)
            .map_err(M::convert_set_payload_error)
    }
}

impl defmt::Format for Response<'_> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        defmt::write!(
            fmt,
            "Response {{ code: {=u8}, content_format_present: {=bool}, content_format: {=u16}, payload_len: {=usize} }}",
            self.code,
            self.content_format.is_some(),
            self.content_format.unwrap_or(0),
            self.payload.len()
        )
    }
}

/// Handler outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome<'a> {
    /// Request path is outside this handler's route.
    Unhandled,
    /// Request was handled without changing settings.
    Handled(Response<'a>),
    /// Request changed one exact leaf.
    Changed {
        /// Changed Miniconf leaf key.
        key: ChangedKey,
        /// CoAP response to send.
        response: Response<'a>,
    },
}

impl Outcome<'_> {
    /// Return the response, if any.
    pub const fn response(&self) -> Option<Response<'_>> {
        match self {
            Self::Unhandled => None,
            Self::Handled(response) | Self::Changed { response, .. } => Some(*response),
        }
    }
}

impl defmt::Format for Outcome<'_> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        match self {
            Self::Unhandled => defmt::write!(fmt, "Outcome::Unhandled"),
            Self::Handled(response) => defmt::write!(fmt, "Outcome::Handled({})", response),
            Self::Changed { key, response } => defmt::write!(
                fmt,
                "Outcome::Changed {{ key: {}, response: {} }}",
                key,
                response
            ),
        }
    }
}

/// Problem details preserved in error responses.
#[derive(defmt::Format, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// URI path option was not valid UTF-8.
    InvalidUriPath,
    /// Accept option could not be decoded as a CoAP uint.
    InvalidAccept,
    /// Content-Format option could not be decoded as a CoAP uint.
    InvalidContentFormat,
    /// More than one Content-Format option was present.
    DuplicateContentFormat,
    /// An unknown critical CoAP option was present.
    UnknownCriticalOption {
        /// CoAP option number.
        number: u16,
    },
    /// URI path had more segments than this handler captured.
    UriPathTooLong,
    /// A response payload exceeded the optional handler adapter buffer.
    PayloadTooLong,
    /// Request had more Accept options than this handler captures.
    TooManyAcceptOptions,
    /// Request path names no static Miniconf resource.
    NotFound {
        /// Depth reached before lookup failed.
        depth: usize,
    },
    /// Request path continues below a leaf.
    TooLong {
        /// Depth of the leaf under which the request continued.
        depth: usize,
    },
    /// Request path names a known branch, but this route handles leaves only.
    NonLeaf {
        /// Depth of the branch resource.
        depth: usize,
    },
    /// Static schema contains the leaf, but runtime state makes it absent.
    Absent {
        /// Depth of the runtime-absent leaf.
        depth: usize,
    },
    /// Runtime access policy denied the operation.
    Access {
        /// Operation being performed.
        op: Operation,
        /// Error text from Miniconf.
        message: &'static str,
    },
    /// Request method is not supported here.
    MethodNotAllowed,
    /// Request `Accept` option does not allow this route's representation.
    NotAcceptable,
    /// Request payload Content-Format is not supported here.
    UnsupportedContentFormat,
    /// Payload could not be decoded.
    BadPayload,
    /// A read-side value serialization failed.
    Serialization,
}

/// CoAP operation being performed.
#[derive(defmt::Format, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Read operation.
    Read,
    /// Write operation.
    Write,
}

/// CoAP handler error.
#[derive(defmt::Format, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Error {
    /// CoAP response code.
    pub code: u8,
    /// Machine-readable problem.
    pub problem: Problem,
}

impl Error {
    pub(crate) const fn new(code: u8, problem: Problem) -> Self {
        Self { code, problem }
    }

    pub(crate) const fn bad_request(problem: Problem) -> Self {
        Self::new(code::BAD_REQUEST, problem)
    }

    pub(crate) const fn request_entity_too_large(problem: Problem) -> Self {
        Self::new(code::REQUEST_ENTITY_TOO_LARGE, problem)
    }

    pub(crate) fn response<'a>(self, buf: &'a mut [u8]) -> Response<'a> {
        #[cfg(feature = "json-core")]
        {
            match problem_json(self.problem, buf) {
                Ok(len) => Response {
                    code: self.code,
                    content_format: Some(format::JSON),
                    payload: &buf[..len],
                },
                Err(_) => Response {
                    code: self.code,
                    content_format: None,
                    payload: b"",
                },
            }
        }
        #[cfg(not(feature = "json-core"))]
        {
            let _ = buf;
            Response {
                code: self.code,
                content_format: None,
                payload: b"",
            }
        }
    }

    #[cfg(feature = "json-core")]
    pub(crate) fn write_json_response_to<M: MutableWritableMessage>(
        self,
        message: &mut M,
        max_len: usize,
    ) -> Result<(), M::UnionError> {
        self.write_problem_to(message, format::JSON, max_len, |error, buf| {
            problem_json(error.problem, buf).ok()
        })
    }

    #[cfg(feature = "cbor")]
    pub(crate) fn cbor_response<'a>(self, buf: &'a mut [u8]) -> Response<'a> {
        match problem_cbor(self, buf) {
            Ok(len) => Response {
                code: self.code,
                content_format: Some(format::CONCISE_PROBLEM_CBOR),
                payload: &buf[..len],
            },
            Err(_) => Response {
                code: self.code,
                content_format: None,
                payload: b"",
            },
        }
    }

    #[cfg(feature = "cbor")]
    pub(crate) fn write_cbor_response_to<M: MutableWritableMessage>(
        self,
        message: &mut M,
        max_len: usize,
    ) -> Result<(), M::UnionError> {
        self.write_problem_to(
            message,
            format::CONCISE_PROBLEM_CBOR,
            max_len,
            |error, buf| problem_cbor(error, buf).ok(),
        )
    }

    #[cfg(any(feature = "json-core", feature = "cbor"))]
    fn write_problem_to<M, F>(
        self,
        message: &mut M,
        content_format: u16,
        max_len: usize,
        encode: F,
    ) -> Result<(), M::UnionError>
    where
        M: MutableWritableMessage,
        F: FnOnce(Self, &mut [u8]) -> Option<usize>,
    {
        message.set_code(M::Code::new(self.code).map_err(M::convert_code_error)?);
        message
            .add_option_uint(
                M::OptionNumber::new(option::CONTENT_FORMAT)
                    .map_err(M::convert_option_number_error)?,
                content_format,
            )
            .map_err(M::convert_add_option_error)?;
        let payload = message
            .payload_mut_with_len(max_len)
            .map_err(M::convert_set_payload_error)?;
        let len = encode(self, payload).unwrap_or(0);
        message.truncate(len).map_err(M::convert_set_payload_error)
    }
}

#[cfg(feature = "json-core")]
fn problem_json(problem: Problem, buf: &mut [u8]) -> Result<usize, fmt::Error> {
    let mut out = SliceWriter::new(buf);
    match problem {
        Problem::InvalidUriPath
        | Problem::InvalidAccept
        | Problem::InvalidContentFormat
        | Problem::DuplicateContentFormat
        | Problem::UriPathTooLong
        | Problem::PayloadTooLong
        | Problem::TooManyAcceptOptions
        | Problem::MethodNotAllowed
        | Problem::NotAcceptable
        | Problem::UnsupportedContentFormat
        | Problem::BadPayload
        | Problem::Serialization => write_kind(&mut out, problem_kind(problem))?,
        Problem::UnknownCriticalOption { number } => {
            write!(
                out,
                "{{\"kind\":\"unknown_critical_option\",\"number\":{number}}}"
            )?;
        }
        Problem::NotFound { depth } => write_depth(&mut out, "not_found", depth)?,
        Problem::TooLong { depth } => write_depth(&mut out, "too_long", depth)?,
        Problem::NonLeaf { depth } => write_depth(&mut out, "non_leaf", depth)?,
        Problem::Absent { depth } => write_depth(&mut out, "absent", depth)?,
        Problem::Access { op, message } => {
            write!(
                out,
                "{{\"kind\":\"access\",\"op\":\"{}\",\"message\":",
                operation_name(op)
            )?;
            write_json_string_truncated(&mut out, message, 1)?;
            out.push('}')?;
        }
    }
    Ok(out.len())
}

#[cfg(feature = "cbor")]
fn problem_cbor(error: Error, buf: &mut [u8]) -> Result<usize, encode::Error<EndOfSlice>> {
    const MINICONF_PROBLEM_KEY: &str = "tag:quartiq.de,2026:miniconf";

    let mut cursor = encode::write::Cursor::new(buf);
    {
        let kind = problem_kind(error.problem);
        let mut encoder = CborEncoder::new(&mut cursor);
        encoder
            .map(3)?
            .int(CborInt::from(-1i8))?
            .str(kind)?
            .int(CborInt::from(-4i8))?
            .u8(error.code)?
            .str(MINICONF_PROBLEM_KEY)?
            .map(problem_detail_len(error.problem))?
            .str("kind")?
            .str(kind)?;

        match error.problem {
            Problem::UnknownCriticalOption { number } => {
                encoder.str("number")?.u16(number)?;
            }
            Problem::NotFound { depth }
            | Problem::TooLong { depth }
            | Problem::NonLeaf { depth }
            | Problem::Absent { depth } => {
                encoder.str("depth")?.u64(depth as u64)?;
            }
            Problem::Access { op, message } => {
                encoder
                    .str("op")?
                    .str(operation_name(op))?
                    .str("message")?
                    .str(message)?;
            }
            _ => {}
        }
    }
    Ok(cursor.position())
}

#[cfg(feature = "cbor")]
const fn problem_detail_len(problem: Problem) -> u64 {
    match problem {
        Problem::UnknownCriticalOption { .. }
        | Problem::NotFound { .. }
        | Problem::TooLong { .. }
        | Problem::NonLeaf { .. }
        | Problem::Absent { .. } => 2,
        Problem::Access { .. } => 3,
        _ => 1,
    }
}

#[cfg(any(feature = "json-core", feature = "cbor"))]
const fn problem_kind(problem: Problem) -> &'static str {
    match problem {
        Problem::InvalidUriPath => "invalid_uri_path",
        Problem::InvalidAccept => "invalid_accept",
        Problem::InvalidContentFormat => "invalid_content_format",
        Problem::DuplicateContentFormat => "duplicate_content_format",
        Problem::UnknownCriticalOption { .. } => "unknown_critical_option",
        Problem::UriPathTooLong => "uri_path_too_long",
        Problem::PayloadTooLong => "payload_too_long",
        Problem::TooManyAcceptOptions => "too_many_accept_options",
        Problem::NotFound { .. } => "not_found",
        Problem::TooLong { .. } => "too_long",
        Problem::NonLeaf { .. } => "non_leaf",
        Problem::Absent { .. } => "absent",
        Problem::Access { .. } => "access",
        Problem::MethodNotAllowed => "method_not_allowed",
        Problem::NotAcceptable => "not_acceptable",
        Problem::UnsupportedContentFormat => "unsupported_content_format",
        Problem::BadPayload => "bad_payload",
        Problem::Serialization => "serialization",
    }
}

#[cfg(any(feature = "json-core", feature = "cbor"))]
const fn operation_name(op: Operation) -> &'static str {
    match op {
        Operation::Read => "read",
        Operation::Write => "write",
    }
}

#[cfg(feature = "json-core")]
struct SliceWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

#[cfg(feature = "json-core")]
impl<'a> SliceWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    const fn len(&self) -> usize {
        self.len
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.len
    }

    fn push(&mut self, value: char) -> fmt::Result {
        self.write_char(value)
    }
}

#[cfg(feature = "json-core")]
impl fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let bytes = value.as_bytes();
        let Some(dst) = self.buf.get_mut(self.len..self.len + bytes.len()) else {
            return Err(fmt::Error);
        };
        dst.copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}

#[cfg(feature = "json-core")]
fn write_kind(out: &mut impl fmt::Write, kind: &str) -> fmt::Result {
    write!(out, "{{\"kind\":\"{}\"}}", kind)
}

#[cfg(feature = "json-core")]
fn write_depth(out: &mut impl fmt::Write, kind: &str, depth: usize) -> fmt::Result {
    write!(out, "{{\"kind\":\"{}\",\"depth\":{}}}", kind, depth)
}

#[cfg(feature = "json-core")]
fn write_json_string_truncated(
    out: &mut SliceWriter<'_>,
    value: &str,
    reserve: usize,
) -> fmt::Result {
    out.push('"')?;
    for byte in value.bytes() {
        let escaped = match byte {
            b'"' => "\\\"",
            b'\\' => "\\\\",
            0x08 => "\\b",
            0x0c => "\\f",
            b'\n' => "\\n",
            b'\r' => "\\r",
            b'\t' => "\\t",
            _ => "",
        };
        if !escaped.is_empty() {
            if out.remaining() <= reserve + 1 || escaped.len() > out.remaining() - reserve - 1 {
                break;
            }
            out.write_str(escaped)?;
            continue;
        }
        match byte {
            0x00..=0x1f => {
                if out.remaining() <= reserve + 1 || 6 > out.remaining() - reserve - 1 {
                    break;
                }
                write!(out, "\\u{:04x}", byte)?;
            }
            _ => {
                if out.remaining() <= reserve + 1 {
                    break;
                }
                out.push(char::from(byte))?;
            }
        }
    }
    out.push('"')
}

impl RenderableOnMinimal for Error {
    type Error<IE: RenderableOnMinimal + fmt::Debug> = IE;

    fn render<M: MinimalWritableMessage>(
        self,
        message: &mut M,
    ) -> Result<(), Self::Error<M::UnionError>> {
        let mut buf = [0; 96];
        self.response(&mut buf).write_to(message)
    }
}

impl From<Infallible> for Error {
    fn from(value: Infallible) -> Self {
        match value {}
    }
}
