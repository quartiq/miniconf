use crate::{LeafKey, MAX_URI_PATH_LENGTH};
use coap_message::{
    Code as _, MessageOption, MinimalWritableMessage, OptionNumber as _, ReadableMessage,
    error::RenderableOnMinimal,
};
use coap_numbers::{code, option};
use core::{convert::Infallible, fmt};
use fmt::Write as _;

type UriPath = heapless::String<MAX_URI_PATH_LENGTH>;

/// Parsed CoAP request data used by cooperative handlers.
#[derive(Debug)]
pub struct RequestParts<'a> {
    pub(crate) code: u8,
    pub(crate) path: UriPath,
    pub(crate) accept: Option<u16>,
    pub(crate) content_format: Option<u16>,
    pub(crate) invalid_option: Option<u16>,
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
            accept,
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
            accept: None,
            content_format: None,
            invalid_option: None,
            payload: message.payload(),
        };

        let mut content_format_seen = false;
        for opt in message.options() {
            match opt.number() {
                option::URI_PATH => {
                    let Some(segment) = opt.value_str() else {
                        return Err(Error::new(code::BAD_OPTION, Problem::InvalidUriPath));
                    };
                    request.push_path_segment(segment)?;
                }
                option::ACCEPT => {
                    if request.accept.is_some() || opt.value().len() > 2 {
                        request.invalid_option.get_or_insert(option::ACCEPT);
                    } else {
                        request.accept = opt.value_uint();
                    }
                }
                option::CONTENT_FORMAT => {
                    if !content_format_seen {
                        content_format_seen = true;
                        // Invalid and excess elective options are ignored.
                        if opt.value().len() <= 2 {
                            request.content_format = opt.value_uint();
                        }
                    }
                }
                option::URI_HOST | option::URI_PORT => {}
                number
                    if option::get_criticality(number) == option::Criticality::Critical
                        && request.invalid_option.is_none() =>
                {
                    request.invalid_option = Some(number);
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

    /// Rooted Miniconf path, joined from URI segments.
    pub fn path(&self) -> &str {
        self.path.as_str()
    }

    /// Request payload.
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    pub(crate) fn accepts(&self, content_format: u16) -> Result<(), Error> {
        if self.accept.is_none_or(|accept| accept == content_format) {
            Ok(())
        } else {
            Err(Error::new(code::NOT_ACCEPTABLE, Problem::NotAcceptable))
        }
    }

    pub(crate) fn check_options(&self) -> Result<(), Error> {
        match self.invalid_option {
            Some(number) => Err(Error::new(code::BAD_OPTION, Problem::BadOption { number })),
            None => Ok(()),
        }
    }

    fn push_path_segment(&mut self, segment: &str) -> Result<(), Error> {
        if segment.len() > 255 || segment.contains('/') {
            return Err(Error::new(code::BAD_OPTION, Problem::InvalidUriPath));
        }
        self.path
            .push('/')
            .map_err(|_| Error::new(code::INTERNAL_SERVER_ERROR, Problem::PathCapacity))?;
        self.path
            .push_str(segment)
            .map_err(|_| Error::new(code::INTERNAL_SERVER_ERROR, Problem::PathCapacity))
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

impl defmt::Format for RequestParts<'_> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        defmt::write!(
            fmt,
            "RequestParts {{ code: {=u8}, path: {=str}, accept_present: {=bool}, accept: {=u16}, content_format_present: {=bool}, content_format: {=u16}, payload_len: {=usize} }}",
            self.code,
            self.path.as_str(),
            self.accept.is_some(),
            self.accept.unwrap_or(0),
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
    /// Freshness in seconds; absence uses the CoAP default of 60 seconds.
    pub max_age: Option<u32>,
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
        if let Some(max_age) = self.max_age {
            message
                .add_option_uint(
                    M::OptionNumber::new(option::MAX_AGE)
                        .map_err(M::convert_option_number_error)?,
                    max_age,
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
    /// Request was handled without a successful assignment.
    Handled(Response<'a>),
    /// Request successfully assigned one exact leaf, possibly its previous value.
    Written {
        /// Assigned Miniconf leaf key.
        key: LeafKey,
        /// CoAP response to send.
        response: Response<'a>,
    },
}

impl<'a> Outcome<'a> {
    /// Return the response, if any.
    pub const fn response(&self) -> Option<Response<'a>> {
        match self {
            Self::Unhandled => None,
            Self::Handled(response) | Self::Written { response, .. } => Some(*response),
        }
    }
}

impl defmt::Format for Outcome<'_> {
    fn format(&self, fmt: defmt::Formatter<'_>) {
        match self {
            Self::Unhandled => defmt::write!(fmt, "Outcome::Unhandled"),
            Self::Handled(response) => defmt::write!(fmt, "Outcome::Handled({})", response),
            Self::Written { key, response } => defmt::write!(
                fmt,
                "Outcome::Written {{ key: {}, response: {} }}",
                key,
                response
            ),
        }
    }
}

/// The reason a request failed.
#[derive(defmt::Format, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// URI path was not UTF-8, exceeded the option length, or contained a separator.
    InvalidUriPath,
    /// An unrecognized, malformed or repeated critical option was present.
    BadOption {
        /// CoAP option number.
        number: u16,
    },
    /// Encoded or resolved path exceeded its fixed storage.
    PathCapacity,
    /// A response exceeded the supplied buffer or fixed schema page budget.
    PayloadTooLong,
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
    /// The reason for the failure.
    pub problem: Problem,
}

impl Error {
    pub(crate) const fn new(code: u8, problem: Problem) -> Self {
        Self { code, problem }
    }

    pub(crate) fn response(self, buf: &mut [u8]) -> Response<'_> {
        let mut out = SliceWriter { buf, len: 0 };
        let len = if write!(out, "{}", self.problem).is_ok() {
            out.len
        } else {
            0
        };
        Response {
            code: self.code,
            content_format: None,
            max_age: Some(0),
            payload: &buf[..len],
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUriPath => f.write_str("invalid Miniconf URI path"),
            Self::BadOption { number } => write!(f, "bad option {number}"),
            Self::PathCapacity => f.write_str("URI path capacity exceeded"),
            Self::PayloadTooLong => f.write_str("response capacity exceeded"),
            Self::NotFound { depth } => write!(f, "unknown key at depth {depth}"),
            Self::TooLong { depth } => write!(f, "path continues below leaf at depth {depth}"),
            Self::NonLeaf { depth } => write!(f, "not a leaf at depth {depth}"),
            Self::Absent { depth } => write!(f, "absent value at depth {depth}"),
            Self::Access { op, message } => write!(f, "{op:?}: {message}"),
            Self::MethodNotAllowed => f.write_str("method not allowed"),
            Self::NotAcceptable => f.write_str("representation not available"),
            Self::UnsupportedContentFormat => f.write_str("unsupported content format"),
            Self::BadPayload => f.write_str("invalid value payload"),
            Self::Serialization => f.write_str("value serialization failed"),
        }
    }
}

struct SliceWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl fmt::Write for SliceWriter<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        let dst = self
            .buf
            .get_mut(self.len..self.len + value.len())
            .ok_or(fmt::Error)?;
        dst.copy_from_slice(value.as_bytes());
        self.len += value.len();
        Ok(())
    }
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
