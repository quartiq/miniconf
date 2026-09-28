use coap_numbers::code;
use defmt::trace;
use miniconf::{
    Schema,
    compact_schema::{SchemaDefs, serialize_schema_page},
};
use serde::Serialize;
use yafnv::Fnv;

use crate::{Error, MAX_SCHEMA_DEFS, Outcome, Problem, RequestParts, Response, format};

const SCHEMA_PROTO: u8 = 1;

/// Schema route backed by a Miniconf schema.
#[derive(Debug, Clone, Copy)]
pub struct SchemaRoute<'a> {
    base: &'a str,
    schema: &'static Schema,
    page_size: usize,
}

impl<'a> SchemaRoute<'a> {
    /// Construct a compact schema route.
    ///
    /// The base path serves a JSON manifest. `base/{page}` serves newline-delimited compact schema
    /// pages of at most `page_size` bytes. Keep this size fixed for the route;
    /// response buffers must accommodate this budget and, for manifest requests,
    /// the manifest itself.
    pub const fn new(base: &'a str, schema: &'static Schema, page_size: usize) -> Self {
        Self {
            base,
            schema,
            page_size,
        }
    }

    /// Handle a schema `GET` request.
    pub fn handle<'b>(
        &self,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b> {
        let Some(resource) = self.resource(request.path()) else {
            trace!("Ignoring non-schema CoAP route request={}", request);
            return Outcome::Unhandled;
        };
        let resource = match self.prepare(request, resource) {
            Ok(resource) => resource,
            Err(error) => return Outcome::Handled(error.response(response_buf)),
        };
        Outcome::Handled(self.render(resource, response_buf))
    }

    pub(crate) fn prepare(
        &self,
        request: &RequestParts<'_>,
        resource: SchemaRequest,
    ) -> Result<SchemaRequest, Error> {
        request.check_options()?;
        if request.code() != code::GET {
            return Err(Error::new(
                code::METHOD_NOT_ALLOWED,
                Problem::MethodNotAllowed,
            ));
        }
        request.accepts(match resource {
            SchemaRequest::Manifest => format::JSON,
            SchemaRequest::Page(_) => format::TEXT,
        })?;
        Ok(resource)
    }

    pub(crate) fn render<'b>(
        &self,
        resource: SchemaRequest,
        response_buf: &'b mut [u8],
    ) -> Response<'b> {
        if response_buf.len() < self.page_size {
            return Error::new(code::INTERNAL_SERVER_ERROR, Problem::PayloadTooLong)
                .response(response_buf);
        }
        let Ok(defs) = SchemaDefs::<MAX_SCHEMA_DEFS>::new(self.schema) else {
            return Error::new(code::INTERNAL_SERVER_ERROR, Problem::Serialization)
                .response(response_buf);
        };

        match resource {
            SchemaRequest::Manifest => self.manifest(&defs, response_buf),
            SchemaRequest::Page(page_index) => self.page(&defs, page_index, response_buf),
        }
    }

    fn manifest<'b, const N: usize>(
        &self,
        defs: &SchemaDefs<N>,
        response_buf: &'b mut [u8],
    ) -> Response<'b> {
        match schema_manifest(defs, self.page_size, response_buf) {
            Ok(len) => Response {
                code: code::CONTENT,
                content_format: Some(format::JSON),
                max_age: None,
                payload: &response_buf[..len],
            },
            Err(problem) => Error::new(code::INTERNAL_SERVER_ERROR, problem).response(response_buf),
        }
    }

    fn page<'b, const N: usize>(
        &self,
        defs: &SchemaDefs<N>,
        page_index: usize,
        response_buf: &'b mut [u8],
    ) -> Response<'b> {
        let mut next = 0;
        for index in 0..=page_index {
            if next >= defs.len() {
                break;
            }
            let page = match serialize_schema_page(defs, next, &mut response_buf[..self.page_size])
            {
                Ok(page) => page,
                Err(_) => {
                    return Error::new(code::INTERNAL_SERVER_ERROR, Problem::PayloadTooLong)
                        .response(response_buf);
                }
            };
            if index == page_index {
                return Response {
                    code: code::CONTENT,
                    content_format: Some(format::TEXT),
                    max_age: None,
                    payload: &response_buf[..page.len],
                };
            }
            next += page.count;
        }

        Error::new(code::NOT_FOUND, Problem::NotFound { depth: 1 }).response(response_buf)
    }

    pub(crate) fn resource(&self, path: &str) -> Option<SchemaRequest> {
        if path == self.base {
            return Some(SchemaRequest::Manifest);
        }
        let suffix = if self.base.is_empty() {
            path.strip_prefix('/')?
        } else {
            path.strip_prefix(self.base)?.strip_prefix('/')?
        };
        (!suffix.is_empty() && !suffix.contains('/'))
            .then(|| parse_usize(suffix).map(SchemaRequest::Page))?
    }
}

/// A prepared compact-schema resource selection.
#[derive(Debug, Clone, Copy)]
pub enum SchemaRequest {
    /// Schema revision and page count.
    Manifest,
    /// A compact-schema page.
    Page(usize),
}

#[derive(Clone, Copy, Debug, Serialize)]
struct SchemaManifest {
    proto: u8,
    epoch: u32,
    schema_rev: u32,
    pages: usize,
}

fn schema_manifest<const N: usize>(
    defs: &SchemaDefs<N>,
    page_size: usize,
    buf: &mut [u8],
) -> Result<usize, Problem> {
    let mut next = 0;
    let mut pages = 0;
    let mut hash = u32::OFFSET_BASIS;

    while next < defs.len() {
        let page = serialize_schema_page(defs, next, &mut buf[..page_size])
            .map_err(|_| Problem::PayloadTooLong)?;
        hash = hash.fnv1a(buf[..page.len].iter().copied());
        next += page.count;
        pages += 1;
    }

    let manifest = SchemaManifest {
        proto: SCHEMA_PROTO,
        epoch: 0,
        schema_rev: hash,
        pages,
    };
    serde_json_core::to_slice(&manifest, buf).map_err(|_| Problem::Serialization)
}

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
