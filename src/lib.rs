#![no_std]
//! # Miniconf
//!
//! Miniconf is a a lightweight utility to manage run-time configurable settings. It allows
//! access and manipulation of struct fields by assigning each field a unique path-like identifier.
//!
//! ## Overview
//!
//! Miniconf uses a [Derive macro](derive.Miniconf.html) to automatically assign unique paths to
//! each setting. All values are transmitted and received in JSON format.
//!
//! With the derive macro, field values can be easily retrieved or modified using a run-time
//! string.
//!
//! ### Supported Protocols
//!
//! Miniconf is designed to be protocol-agnostic. Any means that you have of receiving input from
//! some external source can be used to acquire paths and values for updating settings.
//!
//! There is also an [MQTT-based client](MqttClient) provided to manage settings via the [MQTT
//! protocol](https://mqtt.org) and JSON.
//!
//! ### Example
//! ```
//! use miniconf::{Miniconf, MiniconfAtomic};
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Deserialize, Serialize, MiniconfAtomic, Default)]
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
//! settings.set("sample_rate", b"350").unwrap();
//!
//! // Update filter coefficients.
//! settings.set("filter", b"{\"forward\": 35.6, \"backward\": 0.0}").unwrap();
//!
//! // Update channel gain for channel 0.
//! settings.set("channel_gain/0", b"15").unwrap();
//!
//! // Serialize the current sample rate into the provided buffer.
//! let mut buffer = [0u8; 256];
//! let len = settings.get("sample_rate", &mut buffer).unwrap();
//!
//! assert_eq!(&buffer[..len], b"350");
//! ```
//!
//! ## Features
//! Miniconf supports an MQTT-based client for configuring and managing run-time settings via MQTT.
//! To enable this feature, enable the `mqtt-client` feature.
//!
//! ```no_run
//! #[derive(miniconf::Miniconf, Default, Clone, Debug)]
//! struct Settings {
//!     forward: f32,
//! }
//!
//! // Construct the MQTT client.
//! let mut client: miniconf::MqttClient<_, _, _, 256> = miniconf::MqttClient::new(
//!     std_embedded_nal::Stack::default(),
//!     "example-device",
//!     "quartiq/miniconf-sample",
//!     "127.0.0.1".parse().unwrap(),
//!     std_embedded_time::StandardClock::default(),
//!     Settings::default(),
//! )
//! .unwrap();
//!
//! loop {
//!     // Continually process client updates to detect settings changes.
//!     if client.update().unwrap() {
//!         println!("Settings updated: {:?}", client.settings());
//!     }
//! }
//!
//! ```
//!
//! ### Path iteration
//!
//! Miniconf also allows iteration over all settings paths:
//! ```rust
//! use miniconf::Miniconf;
//!
//! #[derive(Default, Miniconf)]
//! struct Settings {
//!     sample_rate: u32,
//!     update: bool,
//! }
//!
//! let settings = Settings::default();
//!
//!let mut state = [0; 8];
//! for topic in settings.iter::<128>(&mut state).unwrap() {
//!     println!("Discovered topic: `{:?}`", topic);
//! }
//! ```
//!
//! ## Limitations
//!
//! Minconf cannot be used with some of Rust's more complex types. Some unsupported types:
//! * Complex enums
//! * Tuples

#[cfg(feature = "mqtt-client")]
mod mqtt_client;

mod array;
mod option;

/// Provides iteration utilities over [Miniconf] structures.
pub mod iter;

#[cfg(feature = "mqtt-client")]
pub use mqtt_client::MqttClient;

#[cfg(feature = "mqtt-client")]
pub use minimq;

#[cfg(feature = "mqtt-client")]
pub use minimq::embedded_time;

#[doc(hidden)]
pub use serde::{
    de::{Deserialize, DeserializeOwned},
    ser::Serialize,
};

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

    /// The value provided could not be serialized.
    ///
    /// Check that the buffer had sufficient space.
    SerializationFailed,

    /// When indexing into an array, the index provided was out of bounds.
    ///
    /// Check array indices to ensure that bounds for all paths are respected.
    BadIndex,
}

/// Errors that occur during iteration over topic paths.
#[derive(Debug)]
pub enum IterError {
    /// The provided state vector is not long enough.
    InsufficientStateDepth,

    /// The provided topic length is not long enough.
    InsufficientTopicLength,
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
            Error::SerializationFailed => 7,
        }
    }
}

impl From<serde_json_core::de::Error> for Error {
    fn from(err: serde_json_core::de::Error) -> Error {
        Error::Deserialization(err)
    }
}

/// Metadata about a settings structure.
#[derive(Default)]
pub struct MiniconfMetadata {
    /// The maximum length of a topic in the structure.
    pub max_topic_size: usize,

    /// The maximum recursive depth of the structure.
    pub max_depth: usize,
}

/// Derive-able trait for structures that can be mutated using serialized paths and values.
pub trait Miniconf {
    /// Update settings directly from a string path and data.
    ///
    /// # Args
    /// * `path` - The path to update within `settings`.
    /// * `data` - The serialized data making up the contents of the configured value.
    ///
    /// # Returns
    /// The result of the configuration operation.
    fn set(&mut self, path: &str, data: &[u8]) -> Result<(), Error> {
        self.string_set(path.split('/').peekable(), data)
    }

    /// Retrieve a serialized settings value from a string path.
    ///
    /// # Args
    /// * `path` - The path to retrieve.
    /// * `data` - The location to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer for serialization.
    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error> {
        self.string_get(path.split('/').peekable(), data)
    }

    /// Create an iterator to read all possible settings paths.
    ///
    /// # Note
    /// The state vector can be used to resume iteration from a previous point in time. The data
    /// should be zero-initialized if starting iteration for the first time.
    ///
    /// # Template Arguments
    /// * `TS` - The maximum number of bytes to encode a settings path into.
    ///
    /// # Args
    /// * `state` - A state vector to record iteration state in.
    fn iter_settings<'a, const TS: usize>(
        &'a self,
        state: &'a mut [usize],
    ) -> Result<iter::MiniconfIter<'a, Self, TS>, IterError> {
        let metadata = self.get_metadata();

        if TS < metadata.max_topic_size {
            return Err(IterError::InsufficientTopicLength);
        }

        if state.len() < metadata.max_depth {
            return Err(IterError::InsufficientStateDepth);
        }

        Ok(iter::MiniconfIter {
            settings: self,
            state,
        })
    }

    /// Create an iterator to read all possible settings paths.
    ///
    /// # Note
    /// This does not check that the topic size or state vector are large enough. If they are not,
    /// panics may be generated internally by the library.
    ///
    /// # Note
    /// The state vector can be used to resume iteration from a previous point in time. The data
    /// should be zero-initialized if starting iteration for the first time.
    ///
    /// # Template Arguments
    /// * `TS` - The maximum number of bytes to encode a settings path into.
    ///
    /// # Args
    /// * `state` - A state vector to record iteration state in.
    fn unchecked_iter_settings<'a, const TS: usize>(
        &'a self,
        state: &'a mut [usize],
    ) -> iter::MiniconfIter<'a, Self, TS> {
        iter::MiniconfIter {
            settings: self,
            state,
        }
    }

    fn string_set(
        &mut self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error>;

    fn string_get(
        &self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error>;

    /// Get metadata about the settings structure.
    fn get_metadata(&self) -> MiniconfMetadata;

    fn recurse_paths<const TS: usize>(
        &self,
        index: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> Option<()>;
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

            fn string_get(
                &self,
                mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
                value: &mut [u8],
            ) -> Result<usize, Error> {
                if topic_parts.peek().is_some() {
                    return Err(Error::PathTooLong);
                }

                serde_json_core::to_slice(self, value).map_err(|_| Error::SerializationFailed)
            }

            fn get_metadata(&self) -> MiniconfMetadata {
                MiniconfMetadata {
                    // No topic length is needed, as there are no sub-members.
                    max_topic_size: 0,
                    // One index is required for the current element.
                    max_depth: 1,
                }
            }

            // This implementation is the base case for primitives where it will
            // yield once for self, then return None on subsequent calls.
            fn recurse_paths<const TS: usize>(
                &self,
                index: &mut [usize],
                _topic: &mut heapless::String<TS>,
            ) -> Option<()> {
                if index.len() == 0 {
                    // Note: During expected execution paths using `iter()`, the size of the
                    // index stack is checked in advance to make sure this condition doesn't occur.
                    // However, it's possible to happen if the user manually calls `recurse_paths`.
                    unreachable!("Index stack too small");
                }

                let i = index[0];
                index[0] += 1;
                index[1..].iter_mut().for_each(|x| *x = 0);

                if i == 0 {
                    Some(())
                } else {
                    None
                }
            }
        }
    };
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
