#![no_std]
#![warn(missing_docs)]

//! Serve selected `miniconf` trees as CoAP resources.
//!
//! The crate is deliberately sessionless. Applications pass caller-owned settings into
//! cooperative request handlers, and keep ownership of CoAP sockets, message IDs, tokens, routing,
//! retransmission, and unrelated resources.

use core::{convert::Infallible, fmt::Write as _};

use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, OptionNumber as _, ReadableMessage,
};
use coap_numbers::{code, option};
use defmt::{debug, trace, warn};
#[cfg(feature = "cbor")]
use minicbor::{
    decode::{Decoder as CborDecoder, Error as CborDecodeError},
    encode::{self, write::EndOfSlice},
};
#[cfg(feature = "cbor")]
use minicbor_serde::error::{DecodeError as CborDeError, EncodeError as CborSerError};
#[cfg(any(feature = "json-core", feature = "cbor"))]
use miniconf::{DescendError, ResolveError};
use miniconf::{
    Indices, KeyError, Lookup, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    ValueError,
};
#[cfg(feature = "json-core")]
use miniconf::{
    compact_schema::{SchemaDefs, serialize_schema_page},
    json_core,
};
#[cfg(feature = "json-core")]
use serde_json_core::{de::Error as JsonDeError, ser::Error as JsonSerError};

const MAX_ACCEPT_OPTIONS: usize = 4;

/// JSON Content-Format number.
pub const JSON_CONTENT_FORMAT: u16 = 50;

/// CBOR Content-Format number.
pub const CBOR_CONTENT_FORMAT: u16 = 60;

/// CoRE Link Format Content-Format number.
pub const LINK_FORMAT_CONTENT_FORMAT: u16 = 40;

/// Text Content-Format number.
pub const TEXT_CONTENT_FORMAT: u16 = 0;

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

type UriPath = heapless::String<MAX_URI_PATH_LENGTH>;

#[cfg(feature = "coap-handler")]
mod handler;
#[cfg(feature = "coap-handler")]
pub use handler::*;

#[derive(Debug, Clone, Copy, Default)]
struct Accepts {
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
    code: u8,
    path: UriPath,
    accepts: Accepts,
    content_format: Option<u16>,
    invalid_option: Option<InvalidOption>,
    payload: &'a [u8],
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
                    if coap_numbers::option::get_criticality(number)
                        == coap_numbers::option::Criticality::Critical
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

    fn accepts(&self, content_format: u16) -> Result<(), Error> {
        self.accepts.accepts(content_format)
    }

    fn check_options(&self) -> Result<(), Error> {
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

    fn relative_to(&self, base: &str) -> Option<&str> {
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
enum InvalidOption {
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
    const fn new(code: u8, problem: Problem) -> Self {
        Self { code, problem }
    }

    const fn bad_request(problem: Problem) -> Self {
        Self::new(code::BAD_REQUEST, problem)
    }

    const fn request_entity_too_large(problem: Problem) -> Self {
        Self::new(code::REQUEST_ENTITY_TOO_LARGE, problem)
    }

    fn response<'a>(self, buf: &'a mut [u8]) -> Response<'a> {
        match problem_json(self.problem, buf) {
            Ok(len) => Response {
                code: self.code,
                content_format: Some(JSON_CONTENT_FORMAT),
                payload: &buf[..len],
            },
            Err(_) => Response {
                code: self.code,
                content_format: None,
                payload: b"",
            },
        }
    }
}

/// Leaf value route backed by a Miniconf tree.
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct ValueHandler<'a, R> {
    base: &'a str,
    representation: R,
}

/// Const-path-addressed JSON value route.
#[cfg(feature = "json-core")]
pub type ConstPathJsonHandler<'a> = ValueHandler<'a, ConstPathJson>;

/// Const-path-addressed CBOR value route.
#[cfg(feature = "cbor")]
pub type ConstPathCborHandler<'a> = ValueHandler<'a, ConstPathCbor>;

/// URI path segments as Miniconf `ConstPath` keys, with JSON payloads.
#[cfg(feature = "json-core")]
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct ConstPathJson;

/// URI path segments as Miniconf `ConstPath` keys, with CBOR payloads.
#[cfg(feature = "cbor")]
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct ConstPathCbor;

mod private {
    pub trait Sealed {}
}

#[cfg(feature = "json-core")]
impl private::Sealed for ConstPathJson {}
#[cfg(feature = "cbor")]
impl private::Sealed for ConstPathCbor {}

#[cfg(feature = "json-core")]
impl<'a> ValueHandler<'a, ConstPathJson> {
    /// Serve JSON where remaining URI path segments are Miniconf path segments.
    pub const fn const_path_json(base: &'a str) -> Self {
        Self {
            base,
            representation: ConstPathJson,
        }
    }
}

#[cfg(feature = "cbor")]
impl<'a> ValueHandler<'a, ConstPathCbor> {
    /// Serve CBOR where remaining URI path segments are Miniconf path segments.
    pub const fn const_path_cbor(base: &'a str) -> Self {
        Self {
            base,
            representation: ConstPathCbor,
        }
    }
}

impl<'a, R> ValueHandler<'a, R>
where
    R: Representation,
{
    /// Handle a single request using cooperative borrows.
    pub fn handle<'b, Settings>(
        &self,
        request: &RequestParts<'_>,
        settings: &mut Settings,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    {
        let Some(path) = request.relative_to(self.base) else {
            trace!("Ignoring non-Miniconf CoAP route request={}", request);
            return Outcome::Unhandled;
        };
        if let Err(err) = request.check_options() {
            return Outcome::Handled(err.response(response_buf));
        }

        trace!(
            "Handling Miniconf CoAP request base={=str} request={}",
            self.base, request
        );

        match request.code {
            code::GET => self.get(path, &*settings, request, response_buf),
            code::PUT => self.put(path, settings, request, response_buf),
            _ => method_not_allowed(request, response_buf),
        }
    }

    /// Handle a `GET` request with read-only settings bounds.
    pub fn handle_get<'b, Settings>(
        &self,
        request: &RequestParts<'_>,
        settings: &Settings,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b>
    where
        Settings: TreeSchema + TreeSerialize,
    {
        let Some(path) = request.relative_to(self.base) else {
            trace!("Ignoring non-Miniconf CoAP route request={}", request);
            return Outcome::Unhandled;
        };
        if let Err(err) = request.check_options() {
            return Outcome::Handled(err.response(response_buf));
        }

        trace!(
            "Handling Miniconf CoAP GET request base={=str} request={}",
            self.base, request
        );

        if request.code != code::GET {
            return method_not_allowed(request, response_buf);
        }
        self.get(path, settings, request, response_buf)
    }

    /// Handle a `PUT` request with write-only settings bounds.
    pub fn handle_put<'b, Settings>(
        &self,
        request: &RequestParts<'_>,
        settings: &mut Settings,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b>
    where
        Settings: TreeSchema + TreeDeserializeOwned,
    {
        let Some(path) = request.relative_to(self.base) else {
            trace!("Ignoring non-Miniconf CoAP route request={}", request);
            return Outcome::Unhandled;
        };
        if let Err(err) = request.check_options() {
            return Outcome::Handled(err.response(response_buf));
        }

        trace!(
            "Handling Miniconf CoAP PUT request base={=str} request={}",
            self.base, request
        );

        if request.code != code::PUT {
            return method_not_allowed(request, response_buf);
        }
        self.put(path, settings, request, response_buf)
    }

    fn get<'b, Settings>(
        &self,
        path: &str,
        settings: &Settings,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b>
    where
        Settings: TreeSchema + TreeSerialize,
    {
        match self.get_len::<Settings>(path, settings, request, response_buf) {
            Ok(len) => {
                debug!(
                    "Handled Miniconf CoAP GET path={=str} depth={=usize} response_len={=usize}",
                    request.path(),
                    path_depth(path),
                    len
                );
                Outcome::Handled(Response {
                    code: code::CONTENT,
                    content_format: Some(self.representation.content_format()),
                    payload: &response_buf[..len],
                })
            }
            Err(err) => {
                debug!("Rejecting Miniconf CoAP GET err={}", err);
                Outcome::Handled(err.response(response_buf))
            }
        }
    }

    fn get_len<Settings>(
        &self,
        path: &str,
        settings: &Settings,
        request: &RequestParts<'_>,
        response_buf: &mut [u8],
    ) -> Result<usize, Error>
    where
        Settings: TreeSchema + TreeSerialize,
    {
        request.accepts(self.representation.content_format())?;
        let (lookup, state) = self.resolve::<Settings>(path)?;
        if !lookup.schema.is_leaf() {
            return Err(Error::new(
                code::METHOD_NOT_ALLOWED,
                Problem::NonLeaf {
                    depth: lookup.depth,
                },
            ));
        }

        let len = self
            .representation
            .get(settings, &state.as_ref()[..lookup.depth], response_buf)
            .map_err(|err| read_error(err, lookup.depth))?;
        Ok(len)
    }

    fn put<'b, Settings>(
        &self,
        path: &str,
        settings: &mut Settings,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b>
    where
        Settings: TreeSchema + TreeDeserializeOwned,
    {
        match self.put_key::<Settings>(path, settings, request) {
            Ok(key) => Outcome::Changed {
                key,
                response: Response {
                    code: code::CHANGED,
                    content_format: None,
                    payload: b"",
                },
            },
            Err(err) => {
                debug!("Rejecting Miniconf CoAP PUT err={}", err);
                Outcome::Handled(err.response(response_buf))
            }
        }
    }

    fn put_key<Settings>(
        &self,
        path: &str,
        settings: &mut Settings,
        request: &RequestParts<'_>,
    ) -> Result<ChangedKey, Error>
    where
        Settings: TreeSchema + TreeDeserializeOwned,
    {
        match request.content_format {
            Some(format) if format == self.representation.content_format() => {}
            _ => {
                return Err(Error::new(
                    code::UNSUPPORTED_CONTENT_FORMAT,
                    Problem::UnsupportedContentFormat,
                ));
            }
        }

        let (lookup, state) = self.resolve::<Settings>(path)?;
        if !lookup.schema.is_leaf() {
            return Err(Error::new(
                code::METHOD_NOT_ALLOWED,
                Problem::NonLeaf {
                    depth: lookup.depth,
                },
            ));
        }

        self.representation
            .set(settings, &state.as_ref()[..lookup.depth], request.payload)
            .map_err(|err| value_error(err, Operation::Write, lookup.depth))?;
        debug!(
            "Accepted Miniconf CoAP PUT path={=str} depth={=usize} payload_len={=usize}",
            request.path(),
            lookup.depth,
            request.payload.len()
        );
        Ok(state)
    }

    fn resolve<Settings>(&self, path: &str) -> Result<(Lookup, ChangedKey), Error>
    where
        Settings: TreeSchema,
    {
        if Settings::SCHEMA.max_depth() > MAX_DEPTH {
            warn!(
                "Rejecting Miniconf CoAP request because schema depth={=usize} exceeds max_depth={=usize}",
                Settings::SCHEMA.max_depth(),
                MAX_DEPTH
            );
            return Err(Error::new(
                code::REQUEST_ENTITY_TOO_LARGE,
                Problem::UriPathTooLong,
            ));
        }
        let mut state = [0; MAX_DEPTH];
        let lookup = self.representation.resolve::<Settings>(path, &mut state)?;
        Ok((lookup, Indices::new(state, lookup.depth)))
    }
}

/// Complete value representation used by a [`ValueHandler`].
pub trait Representation: private::Sealed {
    /// Serialization error type.
    type SerError;
    /// Deserialization error type.
    type DeError;

    /// CoAP Content-Format used for successful responses and accepted request payloads.
    fn content_format(&self) -> u16;

    /// Resolve a route-relative URI path into a Miniconf schema lookup.
    fn resolve<Settings: TreeSchema>(
        &self,
        path: &str,
        state: &mut [usize],
    ) -> Result<Lookup, Error>;

    /// Serialize a leaf value into the response buffer.
    fn get<Settings: TreeSerialize + ?Sized>(
        &self,
        settings: &Settings,
        keys: &[usize],
        buf: &mut [u8],
    ) -> Result<usize, SerdeError<Self::SerError>>;

    /// Deserialize and set a leaf value from a request payload.
    fn set<Settings: TreeDeserializeOwned + ?Sized>(
        &self,
        settings: &mut Settings,
        keys: &[usize],
        payload: &[u8],
    ) -> Result<(), SerdeError<Self::DeError>>;
}

#[cfg(feature = "json-core")]
impl Representation for ConstPathJson {
    type SerError = JsonSerError;
    type DeError = JsonDeError;

    fn content_format(&self) -> u16 {
        JSON_CONTENT_FORMAT
    }

    fn resolve<Settings: TreeSchema>(
        &self,
        path: &str,
        state: &mut [usize],
    ) -> Result<Lookup, Error> {
        Settings::SCHEMA
            .resolve_into(path, state)
            .map_err(resolve_error)
    }

    fn get<Settings: TreeSerialize + ?Sized>(
        &self,
        settings: &Settings,
        mut keys: &[usize],
        buf: &mut [u8],
    ) -> Result<usize, SerdeError<Self::SerError>> {
        json_core::get_by_keys(settings, &mut keys, buf)
    }

    fn set<Settings: TreeDeserializeOwned + ?Sized>(
        &self,
        settings: &mut Settings,
        mut keys: &[usize],
        payload: &[u8],
    ) -> Result<(), SerdeError<Self::DeError>> {
        json_core::set_by_keys(settings, &mut keys, payload).map(|_| ())
    }
}

#[cfg(feature = "cbor")]
impl Representation for ConstPathCbor {
    type SerError = CborSerError<EndOfSlice>;
    type DeError = CborDeError;

    fn content_format(&self) -> u16 {
        CBOR_CONTENT_FORMAT
    }

    fn resolve<Settings: TreeSchema>(
        &self,
        path: &str,
        state: &mut [usize],
    ) -> Result<Lookup, Error> {
        Settings::SCHEMA
            .resolve_into(path, state)
            .map_err(resolve_error)
    }

    fn get<Settings: TreeSerialize + ?Sized>(
        &self,
        settings: &Settings,
        mut keys: &[usize],
        buf: &mut [u8],
    ) -> Result<usize, SerdeError<Self::SerError>> {
        let mut cursor = encode::write::Cursor::new(buf);
        let mut serializer = minicbor_serde::Serializer::new(&mut cursor);
        settings.serialize_by_key(&mut keys, &mut serializer)?;
        Ok(cursor.position())
    }

    fn set<Settings: TreeDeserializeOwned + ?Sized>(
        &self,
        settings: &mut Settings,
        mut keys: &[usize],
        payload: &[u8],
    ) -> Result<(), SerdeError<Self::DeError>> {
        validate_cbor_payload(payload).map_err(SerdeError::Finalization)?;
        let mut deserializer = minicbor_serde::Deserializer::new(payload);
        settings.deserialize_by_key(&mut keys, &mut deserializer)
    }
}

#[cfg(feature = "cbor")]
fn validate_cbor_payload(payload: &[u8]) -> Result<(), CborDeError> {
    let mut decoder = CborDecoder::new(payload);
    decoder.skip()?;
    if decoder.position() == decoder.input().len() {
        Ok(())
    } else {
        Err(CborDecodeError::message("trailing data").into())
    }
}

/// Schema route backed by `TreeSchema`.
#[cfg(feature = "json-core")]
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct SchemaHandler<'a> {
    base: &'a str,
}

#[cfg(feature = "json-core")]
impl<'a> SchemaHandler<'a> {
    /// Construct a compact paged schema route.
    ///
    /// The base path and `base/0` both serve the first newline-delimited compact schema page.
    pub const fn new(base: &'a str) -> Self {
        Self { base }
    }

    /// Handle a schema `GET` request.
    pub fn handle<'b, Settings>(
        &self,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b>
    where
        Settings: TreeSchema,
    {
        let Some(page_index) = self.page_index(request.path()) else {
            trace!("Ignoring non-schema CoAP route request={}", request);
            return Outcome::Unhandled;
        };
        if let Err(err) = request.check_options() {
            return Outcome::Handled(err.response(response_buf));
        }
        trace!("Handling Miniconf CoAP schema request request={}", request);
        if request.code != code::GET {
            return Outcome::Handled(
                Error::new(code::METHOD_NOT_ALLOWED, Problem::MethodNotAllowed)
                    .response(response_buf),
            );
        }
        if let Err(err) = request.accepts(TEXT_CONTENT_FORMAT) {
            return Outcome::Handled(err.response(response_buf));
        }
        let Ok(defs) = SchemaDefs::<MAX_SCHEMA_DEFS>::new(Settings::SCHEMA) else {
            return Outcome::Handled(
                Error::new(code::INTERNAL_SERVER_ERROR, Problem::Serialization)
                    .response(response_buf),
            );
        };
        let mut next = 0;
        for _ in 0..page_index {
            match serialize_schema_page(&defs, next, response_buf) {
                Ok(page) if page.count != 0 => next += page.count,
                _ => {
                    return Outcome::Handled(
                        Error::new(code::NOT_FOUND, Problem::NotFound { depth: 1 })
                            .response(response_buf),
                    );
                }
            }
        }
        if next >= defs.len() {
            return Outcome::Handled(
                Error::new(code::NOT_FOUND, Problem::NotFound { depth: 1 }).response(response_buf),
            );
        }
        match serialize_schema_page(&defs, next, response_buf) {
            Ok(page) => {
                debug!(
                    "Handled Miniconf CoAP schema GET path={=str} page={=usize} defs={=usize} response_len={=usize}",
                    request.path(),
                    page_index,
                    page.count,
                    page.len
                );
                Outcome::Handled(Response {
                    code: code::CONTENT,
                    content_format: Some(TEXT_CONTENT_FORMAT),
                    payload: &response_buf[..page.len],
                })
            }
            Err(id) => {
                warn!(
                    "Failed to serialize Miniconf CoAP schema path={=str} definition={=usize}",
                    request.path(),
                    id
                );
                Outcome::Handled(
                    Error::request_entity_too_large(Problem::PayloadTooLong).response(response_buf),
                )
            }
        }
    }

    fn page_index(&self, path: &str) -> Option<usize> {
        if path == self.base {
            return Some(0);
        }
        let suffix = if self.base.is_empty() {
            path.strip_prefix('/')?
        } else {
            path.strip_prefix(self.base)?.strip_prefix('/')?
        };
        (!suffix.is_empty() && !suffix.contains('/')).then(|| parse_usize(suffix))?
    }
}

#[cfg(feature = "json-core")]
fn parse_usize(value: &str) -> Option<usize> {
    let mut parsed = 0usize;
    for byte in value.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        parsed = parsed
            .checked_mul(10)?
            .checked_add(usize::from(byte - b'0'))?;
    }
    Some(parsed)
}

fn path_depth(path: &str) -> usize {
    path.as_bytes().iter().filter(|byte| **byte == b'/').count()
}

#[cfg(any(feature = "json-core", feature = "cbor"))]
fn resolve_error(err: ResolveError) -> Error {
    let depth = err.lookup.depth;
    match err.error {
        DescendError::Key(KeyError::NotFound) => {
            Error::new(code::NOT_FOUND, Problem::NotFound { depth })
        }
        DescendError::Key(KeyError::TooLong) => {
            Error::new(code::NOT_FOUND, Problem::TooLong { depth })
        }
        DescendError::Key(KeyError::TooShort) => {
            Error::new(code::METHOD_NOT_ALLOWED, Problem::NonLeaf { depth })
        }
        DescendError::Inner(()) => {
            Error::new(code::REQUEST_ENTITY_TOO_LARGE, Problem::UriPathTooLong)
        }
    }
}

fn read_error<E>(err: SerdeError<E>, depth: usize) -> Error {
    match err {
        SerdeError::Value(ValueError::Key(KeyError::NotFound)) => {
            Error::new(code::NOT_FOUND, Problem::NotFound { depth })
        }
        SerdeError::Value(ValueError::Key(KeyError::TooLong)) => {
            Error::new(code::NOT_FOUND, Problem::TooLong { depth })
        }
        SerdeError::Value(ValueError::Key(KeyError::TooShort)) => {
            Error::new(code::METHOD_NOT_ALLOWED, Problem::NonLeaf { depth })
        }
        SerdeError::Value(ValueError::Absent) => {
            Error::new(code::CONFLICT, Problem::Absent { depth })
        }
        SerdeError::Value(ValueError::Access(message)) => Error::new(
            code::FORBIDDEN,
            Problem::Access {
                op: Operation::Read,
                message,
            },
        ),
        SerdeError::Inner(_) | SerdeError::Finalization(_) => {
            Error::new(code::INTERNAL_SERVER_ERROR, Problem::Serialization)
        }
    }
}

fn value_error<E>(err: SerdeError<E>, op: Operation, depth: usize) -> Error {
    match err {
        SerdeError::Value(ValueError::Key(KeyError::NotFound)) => {
            Error::new(code::NOT_FOUND, Problem::NotFound { depth })
        }
        SerdeError::Value(ValueError::Key(KeyError::TooLong)) => {
            Error::new(code::NOT_FOUND, Problem::TooLong { depth })
        }
        SerdeError::Value(ValueError::Key(KeyError::TooShort)) => {
            Error::new(code::METHOD_NOT_ALLOWED, Problem::NonLeaf { depth })
        }
        SerdeError::Value(ValueError::Absent) => {
            Error::new(code::CONFLICT, Problem::Absent { depth })
        }
        SerdeError::Value(ValueError::Access(message)) => Error::new(
            match op {
                Operation::Read => code::FORBIDDEN,
                Operation::Write => code::UNPROCESSABLE_ENTITY,
            },
            Problem::Access { op, message },
        ),
        SerdeError::Inner(_) | SerdeError::Finalization(_) => {
            Error::new(code::BAD_REQUEST, Problem::BadPayload)
        }
    }
}

fn method_not_allowed<'a>(request: &RequestParts<'_>, response_buf: &'a mut [u8]) -> Outcome<'a> {
    let err = Error::new(code::METHOD_NOT_ALLOWED, Problem::MethodNotAllowed);
    debug!(
        "Rejecting Miniconf CoAP request code={=u8} err={}",
        request.code, err
    );
    Outcome::Handled(err.response(response_buf))
}

fn problem_json(problem: Problem, buf: &mut [u8]) -> Result<usize, core::fmt::Error> {
    let mut out = SliceWriter::new(buf);
    match problem {
        Problem::InvalidUriPath => write_kind(&mut out, "invalid_uri_path")?,
        Problem::InvalidAccept => write_kind(&mut out, "invalid_accept")?,
        Problem::InvalidContentFormat => write_kind(&mut out, "invalid_content_format")?,
        Problem::DuplicateContentFormat => write_kind(&mut out, "duplicate_content_format")?,
        Problem::UnknownCriticalOption { number } => {
            write!(
                out,
                "{{\"kind\":\"unknown_critical_option\",\"number\":{number}}}"
            )?;
        }
        Problem::UriPathTooLong => write_kind(&mut out, "uri_path_too_long")?,
        Problem::PayloadTooLong => write_kind(&mut out, "payload_too_long")?,
        Problem::TooManyAcceptOptions => write_kind(&mut out, "too_many_accept_options")?,
        Problem::NotFound { depth } => write_depth(&mut out, "not_found", depth)?,
        Problem::TooLong { depth } => write_depth(&mut out, "too_long", depth)?,
        Problem::NonLeaf { depth } => write_depth(&mut out, "non_leaf", depth)?,
        Problem::Absent { depth } => write_depth(&mut out, "absent", depth)?,
        Problem::Access { op, message } => {
            write!(
                out,
                "{{\"kind\":\"access\",\"op\":\"{}\",\"message\":",
                match op {
                    Operation::Read => "read",
                    Operation::Write => "write",
                }
            )?;
            write_json_string_truncated(&mut out, message, 1)?;
            out.push('}')?;
        }
        Problem::MethodNotAllowed => write_kind(&mut out, "method_not_allowed")?,
        Problem::NotAcceptable => write_kind(&mut out, "not_acceptable")?,
        Problem::UnsupportedContentFormat => write_kind(&mut out, "unsupported_content_format")?,
        Problem::BadPayload => write_kind(&mut out, "bad_payload")?,
        Problem::Serialization => write_kind(&mut out, "serialization")?,
    }
    Ok(out.len())
}

struct SliceWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

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

    fn push(&mut self, value: char) -> core::fmt::Result {
        self.write_char(value)
    }
}

impl core::fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, value: &str) -> core::fmt::Result {
        let bytes = value.as_bytes();
        let Some(dst) = self.buf.get_mut(self.len..self.len + bytes.len()) else {
            return Err(core::fmt::Error);
        };
        dst.copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }
}

fn write_kind(out: &mut impl core::fmt::Write, kind: &str) -> core::fmt::Result {
    write!(out, "{{\"kind\":\"{}\"}}", kind)
}

fn write_depth(out: &mut impl core::fmt::Write, kind: &str, depth: usize) -> core::fmt::Result {
    write!(out, "{{\"kind\":\"{}\",\"depth\":{}}}", kind, depth)
}

fn write_json_string_truncated(
    out: &mut SliceWriter<'_>,
    value: &str,
    reserve: usize,
) -> core::fmt::Result {
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

impl coap_message::error::RenderableOnMinimal for Error {
    type Error<IE: coap_message::error::RenderableOnMinimal + core::fmt::Debug> = IE;

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

#[cfg(all(test, feature = "json-core"))]
mod tests {
    extern crate std;

    use core::convert::Infallible;

    use coap_message::{MessageOption as _, MinimalWritableMessage, ReadableMessage};
    use coap_numbers::code;
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
                content_format: Some(JSON_CONTENT_FORMAT),
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
        assert_eq!(response.content_format, Some(JSON_CONTENT_FORMAT));
        assert_eq!(response.payload, b"7");

        let mut response = [0; 128];
        let req = request(
            code::PUT,
            &["settings", "number"],
            Some(JSON_CONTENT_FORMAT),
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
            Some(JSON_CONTENT_FORMAT),
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
        assert_eq!(response.content_format, Some(TEXT_CONTENT_FORMAT));
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
        assert_eq!(response.content_format, Some(TEXT_CONTENT_FORMAT));
        assert!(response.payload.starts_with(b"{"));

        let mut response = [0; 512];
        let req = request(code::GET, &["schema", "99"], None, b"");
        let out = handler.handle::<Settings>(&req, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::NOT_FOUND);
        assert_eq!(response.payload, br#"{"kind":"not_found","depth":1}"#);
    }

    #[test]
    fn get_and_put_can_be_used_separately() {
        init_host_logging();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let req = request(code::GET, &["settings", "number"], None, b"");
        let outcome = handler.handle_get(&req, &settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.payload, b"7");

        let mut response = [0; 128];
        let req = request(
            code::PUT,
            &["settings", "number"],
            Some(JSON_CONTENT_FORMAT),
            b"14",
        );
        assert!(matches!(
            handler.handle_put(&req, &mut settings, &mut response),
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
            .uint_option(option::ACCEPT, JSON_CONTENT_FORMAT);
        let request = RequestParts::from_message(&request).unwrap();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let settings = Settings::default();
        let mut response = [0; 128];

        let outcome = handler.handle_get(&request, &settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(response.payload, b"7");

        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::ACCEPT, 0);
        let request = RequestParts::from_message(&request).unwrap();
        let mut response = [0; 128];
        let outcome = handler.handle_get(&request, &settings, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::NOT_ACCEPTABLE);

        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "schema")
            .uint_option(option::ACCEPT, 0)
            .uint_option(option::ACCEPT, JSON_CONTENT_FORMAT);
        let request = RequestParts::from_message(&request).unwrap();
        let handler = SchemaHandler::new("/schema");
        let mut response = [0; 512];
        let outcome = handler.handle::<Settings>(&request, &mut response);
        let response = outcome.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(response.content_format, Some(TEXT_CONTENT_FORMAT));
    }

    #[test]
    fn accept_overflow_is_checked_at_representation_match() {
        init_host_logging();
        let request = TestMessage::new(code::GET)
            .str_option(option::URI_PATH, "settings")
            .str_option(option::URI_PATH, "number")
            .uint_option(option::ACCEPT, JSON_CONTENT_FORMAT)
            .uint_option(option::ACCEPT, 0)
            .uint_option(option::ACCEPT, 1)
            .uint_option(option::ACCEPT, 2)
            .uint_option(option::ACCEPT, 3);
        let request = RequestParts::from_message(&request).unwrap();
        let handler = ConstPathJsonHandler::const_path_json("/settings");
        let settings = Settings::default();
        let mut response = [0; 128];
        let outcome = handler.handle_get(&request, &settings, &mut response);
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
        let outcome = handler.handle_get(&request, &settings, &mut response);
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
            .uint_option(option::CONTENT_FORMAT, JSON_CONTENT_FORMAT)
            .uint_option(option::CONTENT_FORMAT, JSON_CONTENT_FORMAT)
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
        let settings = Settings::default();
        let mut response = [0; 0];
        let req = request(code::GET, &["settings", "number"], None, b"");
        let outcome = handler.handle_get(&req, &settings, &mut response);
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
        assert_eq!(response.content_format, Some(JSON_CONTENT_FORMAT));
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
            .uint_option(option::CONTENT_FORMAT, JSON_CONTENT_FORMAT)
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
            .uint_option(option::ACCEPT, JSON_CONTENT_FORMAT);
        let request = RequestParts::from_message(&request).unwrap();
        let mut response_buf = [0; 128];
        let outcome = route(&request, &mut settings, &mut response_buf);
        let mut response = TestMessage::default();
        outcome.response().unwrap().write_to(&mut response).unwrap();
        assert_eq!(response.code(), code::CONTENT);
        assert_eq!(ReadableMessage::payload(&response), br#"{"ok":true}"#);
        let content_format = response
            .options()
            .find(|opt| opt.number() == option::CONTENT_FORMAT)
            .and_then(|opt| opt.value_uint::<u16>());
        assert_eq!(content_format, Some(JSON_CONTENT_FORMAT));
    }

    #[cfg(feature = "cbor")]
    #[test]
    fn const_path_cbor_get_and_put_leaf() {
        init_host_logging();
        let handler = ConstPathCborHandler::const_path_cbor("/settings");
        let mut settings = Settings::default();
        let mut response = [0; 128];

        let req = request(code::GET, &["settings", "number"], None, b"");
        let out = handler.handle_get(&req, &settings, &mut response);
        let response = out.response().unwrap();
        assert_eq!(response.code, code::CONTENT);
        assert_eq!(response.content_format, Some(CBOR_CONTENT_FORMAT));
        assert_eq!(response.payload, &[7]);

        let mut response = [0; 128];
        let req = request(
            code::PUT,
            &["settings", "number"],
            Some(CBOR_CONTENT_FORMAT),
            &[21],
        );
        let out = handler.handle_put(&req, &mut settings, &mut response);
        assert!(matches!(out, Outcome::Changed { .. }));
        assert_eq!(settings.number, 21);
    }
}
