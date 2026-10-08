#![no_std]
#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

use miniconf::Indices;

/// Maximum depth of a resolved leaf key or discovery traversal.
pub const MAX_DEPTH: usize = 12;

/// Maximum bytes in a captured rooted CoAP URI path.
pub const MAX_URI_PATH_LENGTH: usize = 256;

/// Maximum value, schema, or error response bytes staged by the optional `coap-handler` adapter.
pub const MAX_HANDLER_RESPONSE_LENGTH: usize = 512;

/// Maximum compact schema definitions served by `miniconf_coap`.
pub const MAX_SCHEMA_DEFS: usize = 64;

/// Resolved leaf indices used for reads and successful assignments.
pub type LeafKey = Indices<[usize; MAX_DEPTH]>;

#[cfg(any(feature = "json-core", feature = "cbor", feature = "coap-handler"))]
pub(crate) mod format {
    #[cfg(feature = "json-core")]
    pub const JSON: u16 = match coap_numbers::content_format::from_str("application/json") {
        Some(value) => value,
        None => panic!("unknown CoAP content format"),
    };

    #[cfg(feature = "cbor")]
    pub const CBOR: u16 = match coap_numbers::content_format::from_str("application/cbor") {
        Some(value) => value,
        None => panic!("unknown CoAP content format"),
    };

    #[cfg(feature = "json-core")]
    pub const TEXT: u16 = match coap_numbers::content_format::from_str("text/plain; charset=utf-8")
    {
        Some(value) => value,
        None => panic!("unknown CoAP content format"),
    };
}

#[cfg(feature = "coap-handler")]
mod handler;
mod message;
#[cfg(feature = "json-core")]
mod schema;
mod value;

#[cfg(feature = "coap-handler")]
pub use handler::MiniconfCoapHandler;
#[cfg(all(feature = "coap-handler", feature = "json-core"))]
pub use handler::SchemaCoapHandler;
pub use message::{Error, Operation, Outcome, Problem, RequestParts, Response};
#[cfg(feature = "json-core")]
pub use schema::{SchemaRequest, SchemaRoute};
#[cfg(feature = "cbor")]
pub use value::{Cbor, CborValueRoute};
#[cfg(feature = "json-core")]
pub use value::{Json, JsonValueRoute};
pub use value::{ValueRequest, ValueRoute};

#[cfg(test)]
mod tests {
    use super::*;
    use coap_numbers::code;

    #[test]
    fn diagnostics_preserve_unicode_or_are_empty() {
        let error = Error::new(
            code::UNPROCESSABLE_ENTITY,
            Problem::Access {
                op: Operation::Write,
                message: "échelle 温度",
            },
        );
        let mut buf = [0; 64];
        let response = error.response(&mut buf);
        assert_eq!(response.content_format, None);
        assert_eq!(
            core::str::from_utf8(response.payload).unwrap(),
            "Write: échelle 温度"
        );
        for len in 0..response.payload.len() {
            let mut buf = [0; 64];
            let response = error.response(&mut buf[..len]);
            assert_eq!(response.code, code::UNPROCESSABLE_ENTITY);
            assert!(response.payload.is_empty());
        }
    }
}
