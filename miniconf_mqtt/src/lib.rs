#![no_std]
#![warn(missing_docs)]

//! Async MQTT interface for `miniconf` built on a single long-lived [`minimq::Session`].
//!
//! Limitations:
//! - one MQTT prefix is expected to have one authoritative device publisher
//! - retained manifest, schema, and settings publication is incremental rather than atomic

mod client;
mod message;
mod schema;
#[cfg(test)]
mod tests;

pub use minimq;

pub use client::{ChangedKey, Error, Event, Miniconf, Publisher, Service, ServiceEvent, Startup};

pub(crate) use defmt::{debug, info, warn};

/// Maximum path-state depth supported by `miniconf_mqtt`.
pub const MAX_DEPTH: usize = 12;

pub(crate) const MAX_TOPIC_LENGTH: usize = minimq::TOPIC_CAPACITY;
pub(crate) const RESPONSE_CORRELATION_LENGTH: usize = 32;
pub(crate) const MAX_SCHEMA_DEFS: usize = 64;

/// Payload serialization failed because the provided scratch buffer was too small.
pub(crate) type EncodeError<E> = (bool, E);
