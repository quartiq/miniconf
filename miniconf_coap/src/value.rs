use coap_numbers::code;
#[cfg(any(feature = "json-core", feature = "cbor"))]
use coap_numbers::content_format;
use defmt::{debug, trace, warn};
#[cfg(feature = "cbor")]
use minicbor::{
    decode::{Decoder as CborDecoder, Error as CborDecodeError},
    encode::{self, write::EndOfSlice},
};
#[cfg(feature = "cbor")]
use minicbor_serde::{
    Deserializer as CborDeserializer, Serializer as CborSerializer,
    error::{DecodeError as CborDeError, EncodeError as CborSerError},
};
#[cfg(feature = "json-core")]
use miniconf::json_core;
#[cfg(any(feature = "json-core", feature = "cbor"))]
use miniconf::{DescendError, ResolveError};
use miniconf::{
    Indices, KeyError, Lookup, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    ValueError,
};
#[cfg(feature = "json-core")]
use serde_json_core::{de::Error as JsonDeError, ser::Error as JsonSerError};

use crate::{ChangedKey, Error, MAX_DEPTH, Operation, Outcome, Problem, RequestParts, Response};

/// Leaf value route backed by a Miniconf tree.
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct ValueHandler<'a, R> {
    base: &'a str,
    pub(crate) representation: R,
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
            return Outcome::Handled(self.representation.error_response(err, response_buf));
        }

        trace!(
            "Handling Miniconf CoAP request base={=str} request={}",
            self.base, request
        );

        match request.code() {
            code::GET => self.get(path, &*settings, request, response_buf),
            code::PUT => self.put(path, settings, request, response_buf),
            _ => self.method_not_allowed(request, response_buf),
        }
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
                Outcome::Handled(self.representation.error_response(err, response_buf))
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
                Outcome::Handled(self.representation.error_response(err, response_buf))
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
            .set(settings, &state.as_ref()[..lookup.depth], request.payload())
            .map_err(|err| value_error(err, Operation::Write, lookup.depth))?;
        debug!(
            "Accepted Miniconf CoAP PUT path={=str} depth={=usize} payload_len={=usize}",
            request.path(),
            lookup.depth,
            request.payload().len()
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

    fn method_not_allowed<'b>(
        &self,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b> {
        let err = Error::new(code::METHOD_NOT_ALLOWED, Problem::MethodNotAllowed);
        debug!(
            "Rejecting Miniconf CoAP request code={=u8} err={}",
            request.code(),
            err
        );
        Outcome::Handled(self.representation.error_response(err, response_buf))
    }
}

/// Complete value representation used by a [`ValueHandler`].
#[doc(hidden)]
pub trait Representation: private::Sealed {
    /// Serialization error type.
    type SerError;
    /// Deserialization error type.
    type DeError;

    /// CoAP Content-Format used for successful responses and accepted request payloads.
    fn content_format(&self) -> u16;

    /// CoAP Content-Format used for structured error responses.
    fn error_content_format(&self) -> u16;

    /// Serialize this route's structured error response into the response buffer.
    fn error_response<'a>(&self, error: Error, buf: &'a mut [u8]) -> Response<'a>;

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
        content_format::from_str("application/json").unwrap()
    }

    fn error_content_format(&self) -> u16 {
        content_format::from_str("application/json").unwrap()
    }

    fn error_response<'a>(&self, error: Error, buf: &'a mut [u8]) -> Response<'a> {
        error.response(buf)
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
        content_format::from_str("application/cbor").unwrap()
    }

    fn error_content_format(&self) -> u16 {
        content_format::from_str("application/concise-problem-details+cbor").unwrap()
    }

    fn error_response<'a>(&self, error: Error, buf: &'a mut [u8]) -> Response<'a> {
        error.cbor_response(buf)
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
        let mut serializer = CborSerializer::new(&mut cursor);
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
        let mut deserializer = CborDeserializer::new(payload);
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
