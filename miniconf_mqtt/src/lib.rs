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
use serde::Serialize;

pub use client::{Error, Event, MqttClient};

/// Maximum path-state depth supported by `miniconf_mqtt`.
pub const MAX_DEPTH: usize = 12;

pub(crate) const MAX_TOPIC_LENGTH: usize = 128;
pub(crate) const RESPONSE_CORRELATION_LENGTH: usize = 32;
pub(crate) const MAX_SCHEMA_DEFS: usize = 64;

#[cfg(feature = "compat-settings-ingress")]
const SETTINGS_RECOVERY_QUIESCENCE: Duration = Duration::from_millis(100);

pub(crate) fn json_slice<T: Serialize>(value: &T, buf: &mut [u8]) -> Result<usize, ()> {
    let mut ser = serde_json_core::ser::Serializer::new(buf);
    value.serialize(&mut ser).map_err(|_| ())?;
    Ok(ser.end())
}
