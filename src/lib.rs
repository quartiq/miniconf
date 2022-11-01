#![no_std]
//! # Miniconf
//!
//! Miniconf is a lightweight utility to manage serialization (retrieval) and deserialization
//! (updates, modification) of individual elements of a namespace.
//!
//! ## Overview
//!
//! For structs with named fields, Miniconf uses a [Derive macro](derive.Miniconf.html) to automatically
//! assign a unique path to each item in the namespace. The macro implements the
//! [`Miniconf`](trait.Miniconf.html) trait that exposes access to serialized field values through their path.
//!
//! Elements of homogeneous arrays are similarly accessed through their numeric indices.
//! Structs, arrays, and Options can then be cascaded to construct a multi-level
//! namespace. Control over namespace depth and access to individual elements or
//! atomic updates of complete containers is configured at compile (derive) time.
//!
//! The `Miniconf` implementations for `[T; N]` arrays and `Option<T>` by provides
//! atomic access to their respective inner element(s). Alternatively, [`miniconf::Array`](struct.Array.html) and
//! [`miniconf::Option`](struct.Option.html) can be used to expose the inner element(s) through their
//! `Miniconf` implementations.
//!
//! ### Supported formats
//!
//! The path hierarchy separator is the slash `/`.
//!
//! Values are serialized into and deserialized from JSON format.
//!
//! ### Supported transport protocols
//!
//! Miniconf is designed to be protocol-agnostic. Any means that can receive key-value input from
//! some external source can be used to modify values by path.
//!
//! There is an [MQTT-based client](MqttClient) provided to manage a namespace via the [MQTT
//! protocol](https://mqtt.org) and JSON. See the `mqtt-client` feature.
//!
//! ### Example
//! ```
//! use miniconf::Miniconf;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Deserialize, Serialize, Default)]
//! struct Coefficients {
//!     forward: f32,
//!     backward: f32,
//! }
//!
//! #[derive(Miniconf, Default)]
//! struct Settings {
//!     filter: Coefficients,
//!
//!     // The channel gains are individually configurable.
//!     #[miniconf(defer)]
//!     gain: [f32; 2],
//!
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
//! settings.set("gain/0", b"15").unwrap();
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
//! for topic in settings.iter_paths::<128>(&mut state).unwrap() {
//!     println!("Discovered topic: `{:?}`", topic);
//! }
//! ```
//!
//! ## Nesting
//! Miniconf inherently assumes that (almost) all elements are atomicly updated using a single
//! path.
//!
//! If you would like to nest namespaces, this is supported by explicitly
//! deferring down to the inner Miniconf implementation using the `#[miniconf(defer)]`
//! attribute (compare this to the first example):
//!
//! ```
//! use miniconf::Miniconf;
//! #[derive(Miniconf, Default)]
//! struct Coefficients {
//!     forward: f32,
//!     backward: f32,
//! }
//!
//! #[derive(Miniconf, Default)]
//! struct Settings {
//!     // Explicitly defer downwards into `Coefficient`'s members.
//!     #[miniconf(defer)]
//!     filter: Coefficients,
//!
//!     // The `gain` array is updated in a single value.
//!     gain: [f32; 2],
//! }
//!
//! let mut settings = Settings::default();
//!
//! // Update filter parameters individually.
//! settings.set("filter/forward", b"35.6").unwrap();
//! settings.set("filter/backward", b"0.15").unwrap();
//!
//! // Update the gains simultaneously
//! settings.set("gain", b"[1.0, 2.0]").unwrap()
//! ```
//!
//! ## Limitations
//!
//! Minconf cannot be used with some of Rust's more complex types. Some unsupported types:
//! * Complex enums (other than `Option`)
//! * Tuples

#[cfg(feature = "mqtt-client")]
mod mqtt_client;

mod array;
mod iter;
mod option;

pub use iter::MiniconfIter;

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

pub use array::Array;
pub use option::Option;

pub use serde;
pub use serde_json_core;

pub use derive_miniconf::Miniconf;

pub use heapless;

/// Errors that can occur when using the `Miniconf` API.
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
    ///
    /// Note(deprecated): This error does not occur anymore as of `v0.6`.
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

/// Metadata about a Miniconf namespace.
#[derive(Default)]
pub struct Metadata {
    /// The maximum length of a path in the structure.
    pub max_length: usize,

    /// The maximum depth of the structure.
    pub max_depth: usize,
}

/// Derive-able trait for structures that can be mutated using serialized paths and values.
pub trait Miniconf {
    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The result of the configuration operation.
    fn set(&mut self, path: &str, data: &[u8]) -> Result<(), Error> {
        self.set_path(path.split('/').peekable(), data)
    }

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer for serialization.
    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error> {
        self.get_path(path.split('/').peekable(), data)
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    ///
    /// # Note
    /// To start the iteration from the first path,
    /// the state vector should be initialized with zeros.
    /// The state vector can be used to resume iteration from a previous point in time.
    ///
    /// # Template Arguments
    /// * `TS` - The maximum number of bytes to encode a path into.
    ///
    /// # Args
    /// * `state` - A state vector to record iteration state in.
    fn iter_paths<'a, const TS: usize>(
        &'a self,
        state: &'a mut [usize],
    ) -> Result<iter::MiniconfIter<'a, Self, TS>, IterError> {
        let meta = self.metadata();

        if TS < meta.max_length {
            return Err(IterError::InsufficientTopicLength);
        }

        if state.len() < meta.max_depth {
            return Err(IterError::InsufficientStateDepth);
        }

        Ok(self.unchecked_iter_paths(state))
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    ///
    /// # Note
    /// This does not check that the path size or state vector are large enough. If they are not,
    /// panics may be generated internally by the library.
    ///
    /// # Note
    /// The state vector can be used to resume iteration from a previous point in time. The data
    /// should be zero-initialized if starting iteration for the first time.
    ///
    /// # Template Arguments
    /// * `TS` - The maximum number of bytes to encode a path into.
    ///
    /// # Args
    /// * `state` - A state vector to record iteration state in.
    fn unchecked_iter_paths<'a, const TS: usize>(
        &'a self,
        state: &'a mut [usize],
    ) -> iter::MiniconfIter<'a, Self, TS> {
        iter::MiniconfIter {
            namespace: self,
            state,
        }
    }

    fn set_path(
        &mut self,
        path_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error>;

    fn get_path(
        &self,
        path_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error>;

    fn next_path<const TS: usize>(
        &self,
        state: &mut [usize],
        path: &mut heapless::String<TS>,
    ) -> bool;

    /// Get metadata about the structure.
    fn metadata(&self) -> Metadata;
}
