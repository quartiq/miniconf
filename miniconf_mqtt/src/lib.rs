#![no_std]
#![warn(missing_docs)]

//! Async MQTT interface for `miniconf`.

mod client;
mod pending;
mod protocol;
#[cfg(test)]
mod tests;

pub use client::{Error, MqttClient, State};
pub use minimq;

pub(crate) const MAX_TOPIC_LENGTH: usize = 128;
pub(crate) const MAX_RESPONSE_LENGTH: usize = 128;
pub(crate) const RESPONSE_CORRELATION_LENGTH: usize = 32;
pub(crate) const SEPARATOR: char = '/';
