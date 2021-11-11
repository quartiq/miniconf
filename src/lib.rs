#![no_std]
//! # Miniconf
//!
//! Miniconf is a a lightweight utility to manage run-time configurable settings.
//!
//! ## Overview
//!
//! Miniconf uses a [Derive macro](derive.Miniconf.html) to automatically assign unique paths to
//! each setting. All values are transmitted and received in JSON format.
//!
//! ## Features
//! Miniconf supports an MQTT-based client for configuring and managing run-time settings via MQTT.
//! To enable this feature, enable the `mqtt-client` feature.
//!
//! ## Supported Protocols
//!
//! Miniconf is designed to be protocol-agnostic. Any means that you have of receiving input from
//! some external source can be used to acquire paths and values for updating settings.
//!
//! While Miniconf is platform agnostic, there is an [MQTT-based client](MqttClient) provided to
//! manage settings via the [MQTT protocol](https://mqtt.org).
//!
//! ## Example
//! ```
//! use miniconf::{Miniconf, MiniconfAtomic};
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, MiniconfAtomic, Default)]
//! struct Coefficients {
//!     forward: f32,
//!     backward: f32,
//! }
//!
//! #[derive(Miniconf, Default)]
//! struct Settings {
//!     filter: Coefficients,
//!     channel_gain: [f32; 2],
//!     sample_rate: u32,
//!     force_update: bool,
//! }
//!
//! let mut settings = Settings::default();
//!
//! // Update sample rate.
//! miniconf::update(&mut settings, "sample_rate", b"350").unwrap();
//!
//! // Update filter coefficients.
//! miniconf::update(&mut settings, "filter", b"{\"forward\": 35.6, \"backward\": 0.0}").unwrap();
//!
//! // Update channel gain for channel 0.
//! miniconf::update(&mut settings, "channel_gain/0", b"15").unwrap();
//! ```
//!
//! ## Limitations
//!
//! Minconf cannot be used with some of Rust's more complex types. Some unsupported types:
//! * Complex enums
//! * Tuples

#[cfg(feature = "mqtt-client")]
mod mqtt_client;

#[cfg(feature = "mqtt-client")]
pub use mqtt_client::MqttClient;

#[cfg(feature = "mqtt-client")]
pub use minimq;

#[cfg(feature = "mqtt-client")]
pub use minimq::embedded_time;

#[doc(hidden)]
pub use serde::de::{Deserialize, DeserializeOwned};

pub use serde_json_core;

pub use derive_miniconf::{Miniconf, MiniconfAtomic};

pub use heapless;

/// Errors that occur during settings configuration
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The provided path wasn't found in the structure.
    ///
    /// Double check the provided path to verify that it's valid.
    PathNotFound,

    /// The provided path was valid, but there was trailing data at the end.
    ///
    /// Check the end of the path and remove any excess characters.
    PathTooLong,

    /// The provided path was valid, but did not specify a value fully.
    ///
    /// Double check the ending and add the remainder of the path.
    PathTooShort,

    /// The path provided refers to a member of a configurable structure, but the structure
    /// must be updated all at once.
    ///
    /// Refactor the request to configure the surrounding structure at once.
    AtomicUpdateRequired,

    /// The value provided for configuration could not be deserialized into the proper type.
    ///
    /// Check that the serialized data is valid JSON and of the correct type.
    Deserialization(serde_json_core::de::Error),

    /// When indexing into an array, the index provided was out of bounds.
    ///
    /// Check array indices to ensure that bounds for all paths are respected.
    BadIndex,
}

impl From<Error> for u8 {
    fn from(err: Error) -> u8 {
        match err {
            Error::PathNotFound => 1,
            Error::PathTooLong => 2,
            Error::PathTooShort => 3,
            Error::AtomicUpdateRequired => 4,
            Error::Deserialization(_) => 5,
            Error::BadIndex => 6,
        }
    }
}

impl From<serde_json_core::de::Error> for Error {
    fn from(err: serde_json_core::de::Error) -> Error {
        Error::Deserialization(err)
    }
}

pub trait Miniconf {
    fn string_set(
        &mut self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error>;

    // default implementation is the base case for primitives where it will
    // yield once for self, then return None on subsequent calls. Structs should
    // implement this method if they should be recursed.
    fn recursive_iter<const TS: usize, const VS: usize>(&self, index: &mut [usize], _topic: &mut heapless::String<TS>, value: &mut heapless::String<VS>) -> Option<()>
    where Self: serde::Serialize
    {
        if index.len() == 0 {
            // I don't expect this to happen...
            panic!("index stack too small");
            // return None;
        }

        let i = index[0];
        index[0] += 1;
        index[1..].iter_mut().for_each(|x| *x = 0);

        match i 
        {
            0 => {
                *value = serde_json_core::to_string(&self).unwrap();
                Some(())
            },
            _ => None,
        }
    }
}

/// Convenience function to update settings directly from a string path and data.
///
/// # Note
/// When using prefixes on the path, it is often simpler to call
/// `Settings::string_set(path.peekable(), data)` directly.
///
/// # Args
/// * `settings` - The settings to update
/// * `path` - The path to update within `settings`.
/// * `data` - The serialized data making up the contents of the configured value.
///
/// # Returns
/// The result of the configuration operation.
pub fn update<T: Miniconf>(settings: &mut T, path: &str, data: &[u8]) -> Result<(), Error> {
    settings.string_set(path.split('/').peekable(), data)
}

macro_rules! impl_single {
    ($x:ty) => {
        impl Miniconf for $x {
            fn string_set(
                &mut self,
                mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
                value: &[u8],
            ) -> Result<(), Error> {
                if topic_parts.peek().is_some() {
                    return Err(Error::PathTooLong);
                }
                *self = serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    };
}

impl<T: Miniconf, const N: usize> Miniconf for [T; N] {
    fn string_set(
        &mut self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        let next = topic_parts.next();
        if next.is_none() {
            return Err(Error::PathTooShort);
        }

        // Parse what should be the index value
        let i: usize = serde_json_core::from_str(next.unwrap())
            .or(Err(Error::BadIndex))?
            .0;

        if i >= self.len() {
            return Err(Error::BadIndex);
        }

        self[i].string_set(topic_parts, value)?;

        Ok(())
    }
}

// Implement trait for the primitive types
impl_single!(u8);
impl_single!(u16);
impl_single!(u32);
impl_single!(u64);

impl_single!(i8);
impl_single!(i16);
impl_single!(i32);
impl_single!(i64);

impl_single!(f32);
impl_single!(f64);

impl_single!(usize);
impl_single!(bool);
