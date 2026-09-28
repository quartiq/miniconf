use coap_numbers::code;
#[cfg(feature = "cbor")]
use minicbor::{
    decode::Error as CborDecodeError,
    encode::{self, write::EndOfSlice},
};
#[cfg(feature = "cbor")]
use minicbor_serde::{
    Deserializer as CborDeserializer, Serializer as CborSerializer,
    error::{DecodeError as CborDeError, EncodeError as CborSerError},
};
#[cfg(feature = "json-core")]
use miniconf::json_core;
use miniconf::{DescendError, ResolveError};
use miniconf::{
    Indices, KeyError, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize, ValueError,
};
#[cfg(feature = "json-core")]
use serde_json_core::{de::Error as JsonDeError, ser::Error as JsonSerError};

#[cfg(any(feature = "json-core", feature = "cbor"))]
use crate::format;
use crate::{Error, LeafKey, MAX_DEPTH, Operation, Outcome, Problem, RequestParts, Response};

/// Leaf value route backed by a Miniconf tree.
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct ValueRoute<'a, R> {
    base: &'a str,
    pub(crate) representation: R,
}

/// JSON value route.
#[cfg(feature = "json-core")]
pub type JsonValueRoute<'a> = ValueRoute<'a, Json>;

/// CBOR value route.
#[cfg(feature = "cbor")]
pub type CborValueRoute<'a> = ValueRoute<'a, Cbor>;

/// URI path segments as Miniconf keys, with JSON payloads.
#[cfg(feature = "json-core")]
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct Json;

/// URI path segments as Miniconf keys, with CBOR payloads.
#[cfg(feature = "cbor")]
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct Cbor;

mod private {
    pub trait Sealed {}
}

#[cfg(feature = "json-core")]
impl private::Sealed for Json {}
#[cfg(feature = "cbor")]
impl private::Sealed for Cbor {}

#[cfg(feature = "json-core")]
impl<'a> ValueRoute<'a, Json> {
    /// Serve JSON where remaining URI path segments are Miniconf path segments.
    pub const fn json(base: &'a str) -> Self {
        Self {
            base,
            representation: Json,
        }
    }
}

#[cfg(feature = "cbor")]
impl<'a> ValueRoute<'a, Cbor> {
    /// Serve CBOR where remaining URI path segments are Miniconf path segments.
    pub const fn cbor(base: &'a str) -> Self {
        Self {
            base,
            representation: Cbor,
        }
    }
}

/// A prepared leaf request. PUT has already assigned the value.
#[derive(Debug, Clone)]
pub enum ValueRequest {
    /// Read the leaf when building the response.
    Read(LeafKey),
    /// The payload was successfully assigned; it may equal the previous value.
    Written(LeafKey),
}

impl<R: Representation> ValueRoute<'_, R> {
    /// Handle a request, borrowing settings and response storage from the application.
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
            return Outcome::Unhandled;
        };
        match self.prepare(path, settings, request) {
            Ok(ValueRequest::Written(key)) => Outcome::Written {
                key,
                response: Response {
                    code: code::CHANGED,
                    content_format: None,
                    max_age: None,
                    payload: b"",
                },
            },
            Ok(ValueRequest::Read(key)) => {
                Outcome::Handled(self.read(settings, &key, response_buf))
            }
            Err(error) => Outcome::Handled(error.response(response_buf)),
        }
    }

    pub(crate) fn prepare<Settings: TreeSchema + TreeDeserializeOwned>(
        &self,
        path: &str,
        settings: &mut Settings,
        request: &RequestParts<'_>,
    ) -> Result<ValueRequest, Error> {
        request.check_options()?;
        match request.code() {
            code::GET => request.accepts(self.representation.content_format())?,
            code::PUT => {
                if request.content_format != Some(self.representation.content_format()) {
                    return Err(Error::new(
                        code::UNSUPPORTED_CONTENT_FORMAT,
                        Problem::UnsupportedContentFormat,
                    ));
                }
            }
            _ => {
                return Err(Error::new(
                    code::METHOD_NOT_ALLOWED,
                    Problem::MethodNotAllowed,
                ));
            }
        }
        let mut indices = [0; MAX_DEPTH];
        let lookup = Settings::SCHEMA
            .resolve_into(path, &mut indices)
            .map_err(resolve_error)?;
        if !lookup.schema.is_leaf() {
            return Err(Error::new(
                code::METHOD_NOT_ALLOWED,
                Problem::NonLeaf {
                    depth: lookup.depth,
                },
            ));
        }
        let key = Indices::new(indices, lookup.depth);
        if request.code() == code::GET {
            return Ok(ValueRequest::Read(key));
        }
        self.representation
            .set(settings, key.as_ref(), request.payload())
            .map_err(|error| value_error(error, Operation::Write, lookup.depth))?;
        Ok(ValueRequest::Written(key))
    }

    pub(crate) fn read<'b, Settings: TreeSerialize>(
        &self,
        settings: &Settings,
        key: &LeafKey,
        buf: &'b mut [u8],
    ) -> Response<'b> {
        match self.representation.get(settings, key.as_ref(), buf) {
            Ok(len) => Response {
                code: code::CONTENT,
                content_format: Some(self.representation.content_format()),
                max_age: Some(0),
                payload: &buf[..len],
            },
            Err(error) => value_error(error, Operation::Read, key.as_ref().len()).response(buf),
        }
    }
}

/// Complete value representation used by a [`ValueRoute`].
#[doc(hidden)]
pub trait Representation: private::Sealed {
    /// Serialization error type.
    type SerError;
    /// Deserialization error type.
    type DeError;

    /// CoAP Content-Format used for successful responses and accepted request payloads.
    fn content_format(&self) -> u16;

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
impl Representation for Json {
    type SerError = JsonSerError;
    type DeError = JsonDeError;

    fn content_format(&self) -> u16 {
        format::JSON
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
impl Representation for Cbor {
    type SerError = CborSerError<EndOfSlice>;
    type DeError = CborDeError;

    fn content_format(&self) -> u16 {
        format::CBOR
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
        settings.deserialize_by_key(&mut keys, CompleteCbor(payload))
    }
}

#[cfg(feature = "cbor")]
struct CompleteCbor<'de>(&'de [u8]);

#[cfg(feature = "cbor")]
impl<'de> miniconf::TreeDeserializer<'de> for CompleteCbor<'de> {
    type Ok = ();
    type Error = CborDeError;

    fn deserialize_seed<S: serde::de::DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, ()), SerdeError<Self::Error>> {
        let mut de = CborDeserializer::new(self.0);
        let value = seed.deserialize(&mut de).map_err(SerdeError::Inner)?;
        if de.decoder().position() != self.0.len() {
            return Err(SerdeError::Finalization(
                CborDecodeError::message("trailing data").into(),
            ));
        }
        Ok((value, ()))
    }
}

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
        DescendError::Inner(()) => Error::new(code::INTERNAL_SERVER_ERROR, Problem::PathCapacity),
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
        SerdeError::Inner(_) | SerdeError::Finalization(_) => match op {
            Operation::Read => Error::new(code::INTERNAL_SERVER_ERROR, Problem::Serialization),
            Operation::Write => Error::new(code::BAD_REQUEST, Problem::BadPayload),
        },
    }
}
