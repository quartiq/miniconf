use coap_numbers::code;
use defmt::{debug, trace, warn};
use miniconf::{
    TreeSchema,
    compact_schema::{SchemaDefs, serialize_schema_page},
};
use serde::Serialize;
use yafnv::Fnv;

use crate::{Error, MAX_SCHEMA_DEFS, Outcome, Problem, RequestParts, Response, format};

const SCHEMA_PROTO: u8 = 1;

/// Schema route backed by `TreeSchema`.
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct SchemaRoute<'a> {
    base: &'a str,
}

impl<'a> SchemaRoute<'a> {
    /// Construct a compact schema route.
    ///
    /// The base path serves a JSON manifest. `base/{page}` serves newline-delimited compact schema
    /// pages.
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
        let Some(resource) = self.resource(request.path()) else {
            trace!("Ignoring non-schema CoAP route request={}", request);
            return Outcome::Unhandled;
        };
        if let Err(err) = request.check_options() {
            return Outcome::Handled(err.response(response_buf));
        }
        trace!("Handling Miniconf CoAP schema request request={}", request);
        if request.code() != code::GET {
            return Outcome::Handled(
                Error::new(code::METHOD_NOT_ALLOWED, Problem::MethodNotAllowed)
                    .response(response_buf),
            );
        }
        let Ok(defs) = SchemaDefs::<MAX_SCHEMA_DEFS>::new(Settings::SCHEMA) else {
            return Outcome::Handled(
                Error::new(code::INTERNAL_SERVER_ERROR, Problem::Serialization)
                    .response(response_buf),
            );
        };

        match resource {
            SchemaResource::Manifest => self.manifest(&defs, request, response_buf),
            SchemaResource::Page(page_index) => self.page(&defs, page_index, request, response_buf),
        }
    }

    fn manifest<'b, const N: usize>(
        &self,
        defs: &SchemaDefs<N>,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b> {
        if let Err(err) = request.accepts(format::JSON) {
            return Outcome::Handled(err.response(response_buf));
        }
        match schema_manifest(defs, response_buf) {
            Ok((manifest, len)) => {
                debug!(
                    "Handled Miniconf CoAP schema manifest GET path={=str} pages={=usize} rev={=u32} response_len={=usize}",
                    request.path(),
                    manifest.pages,
                    manifest.schema_rev,
                    len
                );
                Outcome::Handled(Response {
                    code: code::CONTENT,
                    content_format: Some(format::JSON),
                    payload: &response_buf[..len],
                })
            }
            Err(SchemaError::PayloadTooLong(id)) => {
                warn!(
                    "Failed to serialize Miniconf CoAP schema path={=str} definition={=usize}",
                    request.path(),
                    id
                );
                Outcome::Handled(
                    Error::request_entity_too_large(Problem::PayloadTooLong).response(response_buf),
                )
            }
            Err(SchemaError::Serialization) => Outcome::Handled(
                Error::new(code::INTERNAL_SERVER_ERROR, Problem::Serialization)
                    .response(response_buf),
            ),
        }
    }

    fn page<'b, const N: usize>(
        &self,
        defs: &SchemaDefs<N>,
        page_index: usize,
        request: &RequestParts<'_>,
        response_buf: &'b mut [u8],
    ) -> Outcome<'b> {
        if let Err(err) = request.accepts(format::TEXT) {
            return Outcome::Handled(err.response(response_buf));
        }
        let mut next = 0;
        for _ in 0..page_index {
            match serialize_schema_page(defs, next, response_buf) {
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
        match serialize_schema_page(defs, next, response_buf) {
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
                    content_format: Some(format::TEXT),
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

    fn resource(&self, path: &str) -> Option<SchemaResource> {
        if path == self.base {
            return Some(SchemaResource::Manifest);
        }
        let suffix = if self.base.is_empty() {
            path.strip_prefix('/')?
        } else {
            path.strip_prefix(self.base)?.strip_prefix('/')?
        };
        (!suffix.is_empty() && !suffix.contains('/'))
            .then(|| parse_usize(suffix).map(SchemaResource::Page))?
    }
}

enum SchemaResource {
    Manifest,
    Page(usize),
}

#[derive(Clone, Copy, Debug, Serialize)]
struct SchemaManifest {
    proto: u8,
    epoch: u32,
    schema_rev: u32,
    pages: usize,
}

enum SchemaError {
    PayloadTooLong(usize),
    Serialization,
}

fn schema_manifest<const N: usize>(
    defs: &SchemaDefs<N>,
    buf: &mut [u8],
) -> Result<(SchemaManifest, usize), SchemaError> {
    let mut next = 0;
    let mut pages = 0;
    let mut hash = u32::OFFSET_BASIS;

    while next < defs.len() {
        let page = serialize_schema_page(defs, next, buf).map_err(SchemaError::PayloadTooLong)?;
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
    serde_json_core::to_slice(&manifest, buf)
        .map(|len| (manifest, len))
        .map_err(|_| SchemaError::Serialization)
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
