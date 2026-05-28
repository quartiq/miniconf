use coap_numbers::code;
use defmt::{debug, trace, warn};
use miniconf::{
    TreeSchema,
    compact_schema::{SchemaDefs, serialize_schema_page},
};

use crate::{Error, MAX_SCHEMA_DEFS, Outcome, Problem, RequestParts, Response, format};

/// Schema route backed by `TreeSchema`.
#[derive(defmt::Format, Debug, Clone, Copy)]
pub struct SchemaHandler<'a> {
    base: &'a str,
}

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
        if request.code() != code::GET {
            return Outcome::Handled(
                Error::new(code::METHOD_NOT_ALLOWED, Problem::MethodNotAllowed)
                    .response(response_buf),
            );
        }
        if let Err(err) = request.accepts(format::TEXT) {
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
