use core::{fmt::Write as _, iter, marker::PhantomData};

use coap_handler::Attribute;
use coap_message::{MinimalWritableMessage, ReadableMessage};
use coap_numbers::code;
use miniconf::{
    ExactSize, Meta, NodeIter, Schema, TreeDeserializeOwned, TreeSchema, TreeSerialize,
};

#[cfg(feature = "cbor")]
use crate::Cbor;
use crate::{
    Accepts, ChangedKey, Error, InvalidOption, MAX_DEPTH, MAX_HANDLER_RESPONSE_LENGTH, Problem,
    RequestParts, Response, UriPath, ValueRoute, format, value::Representation,
};
#[cfg(feature = "json-core")]
use crate::{Json, SchemaRoute};

/// `coap-handler` adapter for Miniconf leaf value resources.
///
/// This adapter is route-relative: mount it with `coap-handler-implementations`
/// `.below(&["settings"], ...)` and leave URI prefix handling to the ecosystem router.
///
/// It owns its settings because `coap-handler::Handler` has no per-request application context.
/// Use [`ValueRoute`] directly when other components also need cooperative settings access.
#[derive(Debug)]
pub struct MiniconfCoapHandler<Settings, R> {
    settings: Settings,
    values: ValueRoute<'static, R>,
}

#[cfg(feature = "json-core")]
impl<Settings> MiniconfCoapHandler<Settings, Json> {
    /// Create a route-relative JSON Miniconf value handler.
    pub const fn json(settings: Settings) -> Self {
        Self {
            settings,
            values: ValueRoute::json(""),
        }
    }
}

#[cfg(feature = "cbor")]
impl<Settings> MiniconfCoapHandler<Settings, Cbor> {
    /// Create a route-relative CBOR Miniconf value handler.
    pub const fn cbor(settings: Settings) -> Self {
        Self {
            settings,
            values: ValueRoute::cbor(""),
        }
    }
}

/// Request metadata carried from `coap-handler` extraction to response building.
#[doc(hidden)]
#[derive(Debug, Default)]
pub struct CoapHandlerRequest {
    code: u8,
    path: UriPath,
    accepts: Accepts,
    content_format: Option<u16>,
    invalid_option: Option<InvalidOption>,
}

impl CoapHandlerRequest {
    fn from_parts(request: &RequestParts<'_>) -> Self {
        Self {
            code: request.code,
            path: request.path.clone(),
            accepts: request.accepts,
            content_format: request.content_format,
            invalid_option: request.invalid_option,
        }
    }

    fn into_request_parts(self) -> RequestParts<'static> {
        RequestParts {
            code: self.code,
            path: self.path,
            accepts: self.accepts,
            content_format: self.content_format,
            invalid_option: self.invalid_option,
            payload: b"",
        }
    }
}

#[derive(Debug)]
#[doc(hidden)]
pub struct ValueRequest {
    request: CoapHandlerRequest,
    action: ValueAction,
}

#[derive(Debug)]
enum ValueAction {
    Build,
    Changed,
    Error(Error),
}

impl<Settings, R> coap_handler::Handler for MiniconfCoapHandler<Settings, R>
where
    Settings: 'static + TreeSchema + TreeSerialize + TreeDeserializeOwned,
    R: Representation,
{
    type RequestData = ValueRequest;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        let request = RequestParts::from_message(request)?;
        if let Err(err) = request.check_options() {
            return Ok(ValueRequest {
                request: CoapHandlerRequest::default(),
                action: ValueAction::Error(err),
            });
        }
        if request.code() == code::GET {
            return Ok(ValueRequest {
                request: CoapHandlerRequest::from_parts(&request),
                action: ValueAction::Build,
            });
        }
        if request.code() != code::PUT {
            let err = Error::new(code::METHOD_NOT_ALLOWED, Problem::MethodNotAllowed);
            return Ok(ValueRequest {
                request: CoapHandlerRequest::default(),
                action: ValueAction::Error(err),
            });
        }
        // `coap-handler` request data can not borrow the request payload. Apply idempotent PUTs
        // during extraction and carry only the response action, matching the ecosystem handlers.
        let settings = &mut self.settings;
        let path = request.path();
        let action = match self
            .values
            .put_key(path, settings, &request)
            .map(|_key| ValueAction::Changed)
        {
            Ok(action) => action,
            Err(err) => ValueAction::Error(err),
        };
        Ok(ValueRequest {
            request: CoapHandlerRequest::default(),
            action,
        })
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        let _ = request;
        response_estimate(
            self.values.representation.content_format(),
            self.values.representation.error_content_format(),
            MAX_HANDLER_RESPONSE_LENGTH,
        )
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        message: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        match request.action {
            ValueAction::Build => {
                let mut response_buf = [0; MAX_HANDLER_RESPONSE_LENGTH];
                let request = request.request.into_request_parts();
                let settings = &mut self.settings;
                let outcome = self.values.handle(&request, settings, &mut response_buf);
                let response = outcome.response().unwrap_or(Response {
                    code: code::NOT_FOUND,
                    content_format: None,
                    payload: b"",
                });
                response.write_to(message)
            }
            ValueAction::Changed => Response {
                code: code::CHANGED,
                content_format: None,
                payload: b"",
            }
            .write_to(message),
            ValueAction::Error(err) => {
                self.values
                    .representation
                    .write_error(err, message, MAX_HANDLER_RESPONSE_LENGTH)
            }
        }
    }
}

/// `coap-handler` adapter for a Miniconf JSON schema resource.
///
#[cfg(feature = "json-core")]
#[derive(Debug)]
pub struct SchemaCoapHandler {
    schema: &'static Schema,
}

#[cfg(feature = "json-core")]
impl SchemaCoapHandler {
    /// Create a route-relative JSON schema handler.
    pub const fn json(schema: &'static Schema) -> Self {
        Self { schema }
    }
}

#[cfg(feature = "json-core")]
impl coap_handler::Handler for SchemaCoapHandler {
    type RequestData = CoapHandlerRequest;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        RequestParts::from_message(request).map(|request| CoapHandlerRequest::from_parts(&request))
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        let _ = request;
        response_estimate(format::TEXT, format::JSON, MAX_HANDLER_RESPONSE_LENGTH)
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        message: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        let request = request.into_request_parts();
        let mut response_buf = [0; MAX_HANDLER_RESPONSE_LENGTH];
        let outcome = SchemaRoute::new("", self.schema).handle(&request, &mut response_buf);
        let response = outcome.response().unwrap_or(Response {
            code: code::NOT_FOUND,
            content_format: None,
            payload: b"",
        });
        response.write_to(message)
    }
}

impl<Settings, R> coap_handler::Reporting for MiniconfCoapHandler<Settings, R>
where
    Settings: 'static + TreeSchema,
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

fn response_estimate(
    content_format: u16,
    error_content_format: u16,
    payload_capacity: usize,
) -> usize {
    let content_format_option =
        1 + coap_uint_len(content_format).max(coap_uint_len(error_content_format));
    content_format_option + 1 + payload_capacity
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
impl coap_handler::Reporting for SchemaCoapHandler {
    type Record<'res>
        = SchemaRecord
    where
        Self: 'res;
    type Reporter<'res>
        = iter::Once<SchemaRecord>
    where
        Self: 'res;

    fn report(&self) -> Self::Reporter<'_> {
        iter::once(SchemaRecord)
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
                Some(Attribute::Ct(self.content_format)),
                edge_meta
                    .get("rt")
                    .or_else(|| self.node_meta.get("rt"))
                    .map(Attribute::ResourceType),
                edge_meta
                    .get("if")
                    .or_else(|| self.node_meta.get("if"))
                    .map(Attribute::Interface),
                title.map(Attribute::Title),
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
    attrs: [Option<Attribute>; 4],
    pos: usize,
}

impl Iterator for Attributes {
    type Item = Attribute;

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
    type PathElements = iter::Empty<&'static str>;
    type Attributes = core::array::IntoIter<Attribute, 3>;

    fn path(&self) -> Self::PathElements {
        iter::empty()
    }

    fn rel(&self) -> Option<&str> {
        None
    }

    fn attributes(&self) -> Self::Attributes {
        [
            Attribute::Ct(format::JSON),
            Attribute::ResourceType("miniconf.schema"),
            Attribute::Title("Miniconf schema"),
        ]
        .into_iter()
    }
}
