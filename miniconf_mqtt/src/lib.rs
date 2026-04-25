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

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Duration;
pub use minimq;

pub use client::{Error, Event, MqttClient};

/// Maximum path-state depth supported by `miniconf_mqtt`.
pub const MAX_DEPTH: usize = 12;

pub(crate) const MAX_TOPIC_LENGTH: usize = 128;
pub(crate) const RESPONSE_CORRELATION_LENGTH: usize = 32;
pub(crate) const MAX_SCHEMA_DEFS: usize = 64;

/// Payload serialization failed because the provided scratch buffer was too small.
pub(crate) type EncodeError<E> = (bool, E);

#[cfg(feature = "compat-settings-ingress")]
const SETTINGS_RECOVERY_QUIESCENCE: Duration = Duration::from_millis(100);
