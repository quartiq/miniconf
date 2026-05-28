use core::{borrow::BorrowMut, fmt::Write as _, marker::PhantomData};

use coap_message::{MinimalWritableMessage, ReadableMessage};
use coap_numbers::code;
use miniconf::{
    ExactSize, Meta, NodeIter, Schema, TreeDeserializeOwned, TreeSchema, TreeSerialize,
};

#[cfg(feature = "cbor")]
use crate::ConstPathCbor;
use crate::{
    Accepts, ChangedKey, Error, InvalidOption, JSON_CONTENT_FORMAT, MAX_DEPTH,
    MAX_HANDLER_PAYLOAD_LENGTH, Problem, Representation, RequestParts, Response, UriPath,
    ValueHandler,
};
#[cfg(feature = "json-core")]
use crate::{ConstPathJson, SchemaHandler, TEXT_CONTENT_FORMAT};

type HandlerPayload = heapless::Vec<u8, MAX_HANDLER_PAYLOAD_LENGTH>;

/// `coap-handler` adapter for Miniconf leaf value resources.
///
/// This adapter is route-relative: mount it with `coap-handler-implementations`
/// `.below(&["settings"], ...)` and leave URI prefix handling to the ecosystem router.
#[derive(Debug)]
pub struct MiniconfHandler<Storage, Settings, R> {
    settings: Storage,
    values: ValueHandler<'static, R>,
    _settings: PhantomData<Settings>,
}

/// Const-path JSON `coap-handler` adapter.
#[cfg(feature = "json-core")]
pub type ConstPathJsonCoapHandler<'a, Settings> =
    MiniconfHandler<&'a mut Settings, Settings, ConstPathJson>;

/// Const-path CBOR `coap-handler` adapter.
#[cfg(feature = "cbor")]
pub type ConstPathCborCoapHandler<'a, Settings> =
    MiniconfHandler<&'a mut Settings, Settings, ConstPathCbor>;

#[cfg(feature = "json-core")]
impl<Storage, Settings> MiniconfHandler<Storage, Settings, ConstPathJson> {
    /// Create a route-relative JSON Miniconf value handler.
    pub const fn const_path_json(settings: Storage) -> Self {
        Self {
            settings,
            values: ValueHandler::const_path_json(""),
            _settings: PhantomData,
        }
    }
}

#[cfg(feature = "cbor")]
impl<Storage, Settings> MiniconfHandler<Storage, Settings, ConstPathCbor> {
    /// Create a route-relative CBOR Miniconf value handler.
    pub const fn const_path_cbor(settings: Storage) -> Self {
        Self {
            settings,
            values: ValueHandler::const_path_cbor(""),
            _settings: PhantomData,
        }
    }
}

/// Request data carried from `coap-handler` extraction to response building.
#[derive(Debug)]
pub struct CoapHandlerRequest {
    code: u8,
    path: UriPath,
    accepts: Accepts,
    content_format: Option<u16>,
    invalid_option: Option<InvalidOption>,
    payload: HandlerPayload,
}

impl CoapHandlerRequest {
    fn from_message<M>(message: &M) -> Result<Self, Error>
    where
        M: ReadableMessage + ?Sized,
    {
        let request = RequestParts::from_message(message)?;
        let mut payload = HandlerPayload::new();
        payload
            .extend_from_slice(request.payload())
            .map_err(|_| Error::request_entity_too_large(Problem::PayloadTooLong))?;
        Ok(Self {
            code: request.code,
            path: request.path,
            accepts: request.accepts,
            content_format: request.content_format,
            invalid_option: request.invalid_option,
            payload,
        })
    }
}

impl<Storage, Settings, R> coap_handler::Handler for MiniconfHandler<Storage, Settings, R>
where
    Storage: BorrowMut<Settings>,
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    R: Representation,
{
    type RequestData = CoapHandlerRequest;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        CoapHandlerRequest::from_message(request)
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        let _ = request;
        response_estimate(self.values.representation.content_format())
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        message: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        let CoapHandlerRequest {
            code,
            path,
            accepts,
            content_format,
            invalid_option,
            payload,
        } = request;
        let request = RequestParts {
            code,
            path,
            accepts,
            content_format,
            invalid_option,
            payload: payload.as_slice(),
        };
        let mut response_buf = [0; MAX_HANDLER_PAYLOAD_LENGTH];
        let settings = self.settings.borrow_mut();
        // PUT currently mutates while producing the response. The 2.04 response is tiny, but a
        // stricter version should probe/validate, write success, then commit.
        let outcome = self.values.handle(&request, settings, &mut response_buf);
        let response = outcome.response().unwrap_or(Response {
            code: code::NOT_FOUND,
            content_format: None,
            payload: b"",
        });
        response.write_to(message)
    }
}

/// `coap-handler` adapter for a Miniconf JSON schema resource.
#[cfg(feature = "json-core")]
#[derive(Debug)]
pub struct MiniconfSchemaHandler<Settings>(PhantomData<Settings>);

#[cfg(feature = "json-core")]
impl<Settings> MiniconfSchemaHandler<Settings> {
    /// Create a route-relative JSON schema handler.
    pub const fn json() -> Self {
        Self(PhantomData)
    }
}

#[cfg(feature = "json-core")]
impl<Settings> coap_handler::Handler for MiniconfSchemaHandler<Settings>
where
    Settings: TreeSchema,
{
    type RequestData = CoapHandlerRequest;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        CoapHandlerRequest::from_message(request)
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        let _ = request;
        response_estimate(TEXT_CONTENT_FORMAT)
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        message: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        let CoapHandlerRequest {
            code,
            path,
            accepts,
            content_format,
            invalid_option,
            payload,
        } = request;
        let request = RequestParts {
            code,
            path,
            accepts,
            content_format,
            invalid_option,
            payload: payload.as_slice(),
        };
        let mut response_buf = [0; MAX_HANDLER_PAYLOAD_LENGTH];
        let outcome = SchemaHandler::new("").handle::<Settings>(&request, &mut response_buf);
        let response = outcome.response().unwrap_or(Response {
            code: code::NOT_FOUND,
            content_format: None,
            payload: b"",
        });
        response.write_to(message)
    }
}

impl<Storage, Settings, R> coap_handler::Reporting for MiniconfHandler<Storage, Settings, R>
where
    Settings: TreeSchema,
    R: Representation,
{
    type Record<'res>
        = MiniconfRecord<R>
    where
        Self: 'res;
    type Reporter<'res>
        = MiniconfReporter<R>
    where
        Self: 'res;

    fn report(&self) -> Self::Reporter<'_> {
        MiniconfReporter {
            iter: Settings::SCHEMA.nodes::<ChangedKey, MAX_DEPTH>(),
            root_schema: Settings::SCHEMA,
            content_format: self.values.representation.content_format(),
            _representation: PhantomData,
        }
    }
}

fn response_estimate(content_format: u16) -> usize {
    let content_format_option =
        1 + coap_uint_len(content_format).max(coap_uint_len(JSON_CONTENT_FORMAT));
    content_format_option + 1 + MAX_HANDLER_PAYLOAD_LENGTH
}

const fn coap_uint_len(value: u16) -> usize {
    if value == 0 {
        0
    } else if value <= u8::MAX as u16 {
        1
    } else {
        2
    }
}

#[cfg(feature = "json-core")]
impl<Settings> coap_handler::Reporting for MiniconfSchemaHandler<Settings>
where
    Settings: TreeSchema,
{
    type Record<'res>
        = SchemaRecord
    where
        Self: 'res;
    type Reporter<'res>
        = core::iter::Once<SchemaRecord>
    where
        Self: 'res;

    fn report(&self) -> Self::Reporter<'_> {
        core::iter::once(SchemaRecord)
    }
}

/// Iterator over `coap-handler` discovery records for Miniconf leaves.
pub struct MiniconfReporter<R> {
    iter: ExactSize<NodeIter<ChangedKey, MAX_DEPTH>>,
    root_schema: &'static Schema,
    content_format: u16,
    _representation: PhantomData<R>,
}

impl<R> Iterator for MiniconfReporter<R> {
    type Item = MiniconfRecord<R>;

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.iter.next()?.ok()?;
        let (edge_meta, node_meta) = self.root_schema.get_meta(key.as_ref()).ok()?;
        Some(MiniconfRecord {
            key,
            root_schema: self.root_schema,
            edge_meta,
            node_meta,
            content_format: self.content_format,
            _representation: PhantomData,
        })
    }
}

/// A `coap-handler` discovery record for one Miniconf leaf.
pub struct MiniconfRecord<R> {
    key: ChangedKey,
    root_schema: &'static Schema,
    edge_meta: Option<&'static Meta>,
    node_meta: &'static Meta,
    content_format: u16,
    _representation: PhantomData<R>,
}

impl<R> coap_handler::Record for MiniconfRecord<R> {
    type PathElement = DiscoveryPathElement;
    type PathElements = PathSegments;
    type Attributes = Attributes;

    fn path(&self) -> Self::PathElements {
        PathSegments {
            schema: self.root_schema,
            root: self.key,
            depth: 0,
        }
    }

    fn rel(&self) -> Option<&str> {
        None
    }

    fn attributes(&self) -> Self::Attributes {
        let edge_meta = self.edge_meta.unwrap_or(&Meta::EMPTY);
        let title = edge_meta
            .get("title")
            .or_else(|| self.node_meta.get("title"))
            .or_else(|| edge_meta.get("doc"))
            .or_else(|| self.node_meta.get("doc"));
        Attributes {
            attrs: [
                Some(coap_handler::Attribute::Ct(self.content_format)),
                edge_meta
                    .get("rt")
                    .or_else(|| self.node_meta.get("rt"))
                    .map(coap_handler::Attribute::ResourceType),
                edge_meta
                    .get("if")
                    .or_else(|| self.node_meta.get("if"))
                    .map(coap_handler::Attribute::Interface),
                title.map(coap_handler::Attribute::Title),
            ],
            pos: 0,
        }
    }
}

/// Iterator over URI path segments for a Miniconf discovery record.
pub struct PathSegments {
    schema: &'static Schema,
    root: ChangedKey,
    depth: usize,
}

impl Iterator for PathSegments {
    type Item = DiscoveryPathElement;

    fn next(&mut self) -> Option<Self::Item> {
        let index = *self.root.as_ref().get(self.depth)?;
        let internal = self.schema.internal()?;
        let segment = if let Some(name) = internal.get_name(index) {
            DiscoveryPathElement::Name(name)
        } else {
            let mut segment = heapless::String::new();
            write!(segment, "{index}").ok()?;
            DiscoveryPathElement::Index(segment)
        };
        self.schema = internal.get_schema(index);
        self.depth += 1;
        Some(segment)
    }
}

/// Path element used by Miniconf `.well-known/core` discovery records.
pub enum DiscoveryPathElement {
    /// Borrowed schema path name.
    Name(&'static str),
    /// Numeric schema path element.
    Index(heapless::String<20>),
}

impl AsRef<str> for DiscoveryPathElement {
    fn as_ref(&self) -> &str {
        match self {
            Self::Name(name) => name,
            Self::Index(index) => index,
        }
    }
}

/// Iterator over CoRE Link Format attributes for a Miniconf discovery record.
pub struct Attributes {
    attrs: [Option<coap_handler::Attribute>; 4],
    pos: usize,
}

impl Iterator for Attributes {
    type Item = coap_handler::Attribute;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(attr) = self.attrs.get(self.pos).copied() {
            self.pos += 1;
            if attr.is_some() {
                return attr;
            }
        }
        None
    }
}

/// `coap-handler` discovery record for the Miniconf schema resource.
#[cfg(feature = "json-core")]
pub struct SchemaRecord;

#[cfg(feature = "json-core")]
impl coap_handler::Record for SchemaRecord {
    type PathElement = &'static str;
    type PathElements = core::iter::Empty<&'static str>;
    type Attributes = core::array::IntoIter<coap_handler::Attribute, 3>;

    fn path(&self) -> Self::PathElements {
        core::iter::empty()
    }

    fn rel(&self) -> Option<&str> {
        None
    }

    fn attributes(&self) -> Self::Attributes {
        [
            coap_handler::Attribute::Ct(TEXT_CONTENT_FORMAT),
            coap_handler::Attribute::ResourceType("miniconf.schema"),
            coap_handler::Attribute::Title("Miniconf schema"),
        ]
        .into_iter()
    }
}
