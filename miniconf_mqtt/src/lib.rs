#![no_std]
#![warn(missing_docs)]

//! Async MQTT interface for `miniconf` built on a single long-lived [`minimq::Session`].

mod client;
mod json;
mod message;
mod schema;
#[cfg(test)]
mod tests;

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Duration;
pub use minimq;

pub use client::{Error, MqttClient, State};

pub(crate) const MAX_TOPIC_LENGTH: usize = 128;
pub(crate) const MAX_PAYLOAD_LENGTH: usize = 512;
pub(crate) const RESPONSE_CORRELATION_LENGTH: usize = 32;
pub(crate) const SEPARATOR: char = '/';
pub(crate) const MAX_SCHEMA_DEFS: usize = 128;

#[cfg(feature = "compat-settings-ingress")]
const SETTINGS_RECOVERY_QUIESCENCE: Duration = Duration::from_millis(100);
