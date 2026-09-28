use core::fmt::Write as _;
#[cfg(feature = "json-core")]
use core::iter;

use coap_handler::Attribute;
use coap_message::{MinimalWritableMessage, ReadableMessage};
use coap_numbers::code;
use miniconf::{
    ExactSize, Meta, NodeIter, Schema, TreeDeserializeOwned, TreeSchema, TreeSerialize,
};

#[cfg(feature = "cbor")]
use crate::Cbor;
use crate::{
    Error, LeafKey, MAX_DEPTH, MAX_HANDLER_RESPONSE_LENGTH, RequestParts, Response, ValueRoute,
    value::{Representation, ValueRequest},
};
#[cfg(feature = "json-core")]
use crate::{Json, Problem, SchemaRoute, format, schema::SchemaRequest};

/// `coap-handler` adapter for Miniconf leaf value resources.
///
/// This adapter is route-relative: mount it with `coap-handler-implementations`
/// `.below(&["settings"], ...)` and leave URI prefix handling to the ecosystem router.
///
/// PUT assigns during request extraction and returns [`ValueRequest::Written`].
/// Applications can react then; response construction never repeats the assignment.
/// Use [`ValueRoute`] when the application already owns request dispatch and settings.
#[derive(Debug)]
pub struct MiniconfCoapHandler<Settings, R> {
    settings: Settings,
    values: ValueRoute<'static, R>,
}

#[cfg(feature = "json-core")]
impl<Settings: TreeSchema> MiniconfCoapHandler<Settings, Json> {
    /// Create a route-relative JSON Miniconf value handler.
    ///
    /// # Panics
    /// If the schema exceeds [`MAX_DEPTH`], the discovery iterator's capacity.
    pub const fn json(settings: Settings) -> Self {
        assert!(
            Settings::SCHEMA.max_depth() <= MAX_DEPTH,
            "CoAP schema exceeds MAX_DEPTH"
        );
        Self {
            settings,
            values: ValueRoute::json(""),
        }
    }
}

#[cfg(feature = "cbor")]
impl<Settings: TreeSchema> MiniconfCoapHandler<Settings, Cbor> {
    /// Create a route-relative CBOR Miniconf value handler.
    ///
    /// # Panics
    /// If the schema exceeds [`MAX_DEPTH`], the discovery iterator's capacity.
    pub const fn cbor(settings: Settings) -> Self {
        assert!(
            Settings::SCHEMA.max_depth() <= MAX_DEPTH,
            "CoAP schema exceeds MAX_DEPTH"
        );
        Self {
            settings,
            values: ValueRoute::cbor(""),
        }
    }
}

impl<Settings, R> MiniconfCoapHandler<Settings, R> {
    /// Borrow the settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Mutably borrow the settings.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    /// Recover the settings.
    pub fn into_inner(self) -> Settings {
        self.settings
    }
}

impl<Settings, R> coap_handler::Handler for MiniconfCoapHandler<Settings, R>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
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
        self.values
            .prepare(request.path(), &mut self.settings, &request)
    }

    fn estimate_length(&mut self, request: &Self::RequestData) -> usize {
        match request {
            ValueRequest::Written(_) => 0,
            ValueRequest::Read(_) => response_estimate(self.values.representation.content_format()),
        }
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        message: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        match request {
            ValueRequest::Read(key) => {
                let mut buf = [0; MAX_HANDLER_RESPONSE_LENGTH];
                self.values
                    .read(&self.settings, &key, &mut buf)
                    .write_to(message)
            }
            ValueRequest::Written(_) => Response {
                code: code::CHANGED,
                content_format: None,
                max_age: None,
                payload: b"",
            }
            .write_to(message),
        }
    }
}

/// `coap-handler` adapter for a Miniconf JSON schema resource.
///
#[cfg(feature = "json-core")]
#[derive(Debug)]
pub struct SchemaCoapHandler {
    route: SchemaRoute<'static>,
}

#[cfg(feature = "json-core")]
impl SchemaCoapHandler {
    /// Create a route-relative JSON schema handler.
    pub const fn json(schema: &'static Schema) -> Self {
        Self {
            route: SchemaRoute::new("", schema, MAX_HANDLER_RESPONSE_LENGTH),
        }
    }
}

#[cfg(feature = "json-core")]
impl coap_handler::Handler for SchemaCoapHandler {
    type RequestData = SchemaRequest;
    type ExtractRequestError = Error;
    type BuildResponseError<M: MinimalWritableMessage> = M::UnionError;

    fn extract_request_data<M: ReadableMessage>(
        &mut self,
        request: &M,
    ) -> Result<Self::RequestData, Self::ExtractRequestError> {
        let request = RequestParts::from_message(request)?;
        let resource = self
            .route
            .resource(request.path())
            .ok_or(Error::new(code::NOT_FOUND, Problem::NotFound { depth: 0 }))?;
        self.route.prepare(&request, resource)
    }

    fn estimate_length(&mut self, _: &Self::RequestData) -> usize {
        response_estimate(format::JSON)
    }

    fn build_response<M: coap_message::MutableWritableMessage>(
        &mut self,
        message: &mut M,
        request: Self::RequestData,
    ) -> Result<(), Self::BuildResponseError<M>> {
        let mut response_buf = [0; MAX_HANDLER_RESPONSE_LENGTH];
        self.route
            .render(request, &mut response_buf)
            .write_to(message)
    }
}

impl<Settings, R> coap_handler::Reporting for MiniconfCoapHandler<Settings, R>
where
    Settings: TreeSchema,
    R: Representation,
{
    type Record<'res>
        = MiniconfRecord
    where
        Self: 'res;
    type Reporter<'res>
        = MiniconfReporter
    where
        Self: 'res;

    fn report(&self) -> Self::Reporter<'_> {
        MiniconfReporter {
            iter: Settings::SCHEMA.nodes::<LeafKey, MAX_DEPTH>(),
            root_schema: Settings::SCHEMA,
            content_format: self.values.representation.content_format(),
        }
    }
}

fn response_estimate(content_format: u16) -> usize {
    let format_len = match content_format {
        0 => 0,
        1..=255 => 1,
        _ => 2,
    };
    // Content-Format, Max-Age: 0, payload marker, payload.
    3 + format_len + MAX_HANDLER_RESPONSE_LENGTH
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
pub struct MiniconfReporter {
    iter: ExactSize<NodeIter<LeafKey, MAX_DEPTH>>,
    root_schema: &'static Schema,
    content_format: u16,
}

impl Iterator for MiniconfReporter {
    type Item = MiniconfRecord;

    fn next(&mut self) -> Option<Self::Item> {
        let key = self.iter.next()?.expect("validated discovery depth");
        let (edge_meta, node_meta) = self
            .root_schema
            .get_meta(key.as_ref())
            .expect("schema-generated key");
        Some(MiniconfRecord {
            key,
            root_schema: self.root_schema,
            edge_meta,
            node_meta,
            content_format: self.content_format,
        })
    }
}

/// A `coap-handler` discovery record for one Miniconf leaf.
pub struct MiniconfRecord {
    key: LeafKey,
    root_schema: &'static Schema,
    edge_meta: Option<&'static Meta>,
    node_meta: &'static Meta,
    content_format: u16,
}

impl coap_handler::Record for MiniconfRecord {
    type PathElement = DiscoveryPathElement;
    type PathElements = PathSegments;
    type Attributes = core::iter::Flatten<core::array::IntoIter<Option<Attribute>, 4>>;

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
        [
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
        ]
        .into_iter()
        .flatten()
    }
}

/// Iterator over URI path segments for a Miniconf discovery record.
pub struct PathSegments {
    schema: &'static Schema,
    root: LeafKey,
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
