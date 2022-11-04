#![no_std]
//! # Miniconf
//!
//! Miniconf is a lightweight utility to manage serialization (retrieval) and deserialization
//! (updates, modification) of individual elements of a namespace by path.
//!
//! ### Example
//!
//! ```
//! use miniconf::{Miniconf, IterError, Error};
//! use serde::{Serialize, Deserialize};
//! use heapless::{String, Vec};
//!
//! #[derive(Deserialize, Serialize, Copy, Clone, Default, Miniconf)]
//! struct Inner {
//!     a: i32,
//!     b: i32,
//! }
//!
//! #[derive(Miniconf, Default)]
//! struct Settings {
//!     // Primitive access by name
//!     foo: bool,
//!     bar: f32,
//!
//!     // Atomic struct
//!     struct_: Inner,
//!
//!     // Inner access by name
//!     #[miniconf(defer)]
//!     struct_defer: Inner,
//!
//!     // Atomic array
//!     array: [i32; 2],
//!
//!     // Access by index
//!     #[miniconf(defer)]
//!     array_defer: [i32; 2],
//!
//!     // Inner access by index and then name
//!     #[miniconf(defer)]
//!     array_miniconf: miniconf::Array<Inner, 2>,
//!
//!     // Atomic
//!     option: core::option::Option<i32>,
//!
//!     // Run-time optional
//!     #[miniconf(defer)]
//!     option_defer: core::option::Option<i32>,
//!
//!     // Run-time optional with inner access by name
//!     #[miniconf(defer)]
//!     option_miniconf: miniconf::Option<Inner>
//! }
//!
//! let mut settings = Settings::default();
//!
//! // Setting/deserializing elements by paths
//! settings.set("foo", b"true")?;
//! settings.set("bar", b"2.0")?;
//!
//! settings.set("struct_", br#"{"a": 3, "b": 3}"#)?;
//! settings.set("struct_defer/a", b"4")?;
//!
//! settings.set("array", b"[6, 6]")?;
//! settings.set("array_defer/0", b"7")?;
//! settings.set("array_miniconf/1/b", b"11")?;
//!
//! settings.set("option", b"12")?;
//! settings.set("option", b"null")?;
//! settings.set("option_defer", b"13").unwrap_err();
//! settings.option_defer = Some(0);
//! settings.set("option_defer", b"13")?;
//! settings.set("option_miniconf/a", b"14").unwrap_err();
//! *settings.option_miniconf = Some(Inner::default());
//! settings.set("option_miniconf/a", b"14")?;
//!
//! // Getting/serializing elements by path
//! let mut buffer = [0; 256];
//! let len = settings.get("option_miniconf/a", &mut buffer)?;
//! assert_eq!(&buffer[..len], b"14");
//!
//! let paths = Settings::iter_paths::<3, 32>().unwrap();
//! assert_eq!(paths.skip(15).next(), Some("option_miniconf/b".into()));
//!
//! # Ok::<(), miniconf::Error>(())
//! ```
//!
//! ## Overview
//!
//! For structs with named fields, Miniconf offers a [derive macro](derive.Miniconf.html) to automatically
//! assign a unique path to each item in the namespace. The macro implements the
//! [Miniconf] trait that exposes access to serialized field values through their path.
//!
//! Elements of homogeneous arrays are similarly accessed through their numeric indices.
//! Structs, arrays, and Options can then be cascaded to construct a multi-level
//! namespace. Control over namespace depth and access to individual elements or
//! atomic updates of complete containers is configured at compile (derive) time using
//! the `#[miniconf(defer)]` attribute.
//!
//! The [Miniconf] implementations for [core::array] and [core::option::Option] by provide
//! atomic access to their respective inner element(s). The alternatives [Array] and
//! [Option] have [Miniconf] implementations that expose deep access into the
//! inner element(s) through their respective inner [Miniconf] implementations.
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
//! ## Limitations
//!
//! Minconf cannot be used with some of Rust's more complex types. Some unsupported types are:
//! * Complex enums other than [core::option::Option] and [Option]
//! * Tuples
//!
//! ## Features
//!
//! Miniconf supports an MQTT-based client for configuring and managing run-time settings via MQTT.
//! To enable this feature, enable the `mqtt-client` feature. See the example in [MqttClient].

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
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

    /// The value provided for configuration could not be deserialized into the proper type.
    ///
    /// Check that the serialized data is valid JSON and of the correct type.
    Deserialization(serde_json_core::de::Error),

    /// The value provided could not be serialized.
    ///
    /// Check that the buffer had sufficient space.
    Serialization(serde_json_core::ser::Error),

    /// When indexing into an array, the index provided was out of bounds.
    ///
    /// Check array indices to ensure that bounds for all paths are respected.
    BadIndex,

    /// The path does not exist at runtime.
    ///
    /// This is the case if a deferred `core::option::Option` or `miniconf::Option`
    /// is `None` at runtime.
    PathAbsent,
}

/// Errors that occur during iteration over topic paths.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IterError {
    /// The provided state vector is not long enough.
    PathDepth,

    /// The provided topic length is not long enough.
    PathLength,
}

impl From<Error> for u8 {
    fn from(err: Error) -> u8 {
        match err {
            Error::PathNotFound => 1,
            Error::PathTooLong => 2,
            Error::PathTooShort => 3,
            Error::Deserialization(_) => 5,
            Error::BadIndex => 6,
            Error::Serialization(_) => 7,
            Error::PathAbsent => 8,
        }
    }
}

impl From<serde_json_core::de::Error> for Error {
    fn from(err: serde_json_core::de::Error) -> Error {
        Error::Deserialization(err)
    }
}

impl From<serde_json_core::ser::Error> for Error {
    fn from(err: serde_json_core::ser::Error) -> Error {
        Error::Serialization(err)
    }
}

/// Metadata about a Miniconf namespace.
#[non_exhaustive]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// The maximum length of a path.
    pub max_length: usize,

    /// The maximum path depth.
    pub max_depth: usize,

    /// The number of paths.
    pub count: usize,
}

/// Helper trait for `core::iter::Peekable`.
pub trait Peekable: core::iter::Iterator {
    fn peek(&mut self) -> core::option::Option<&Self::Item>;
}

impl<I: core::iter::Iterator> Peekable for core::iter::Peekable<I> {
    fn peek(&mut self) -> core::option::Option<&Self::Item> {
        core::iter::Peekable::peek(self)
    }
}

/// Derive-able trait for structures that can be mutated using serialized paths and values.
pub trait Miniconf {
    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element with '/' as the separator.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an `Error`.
    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error> {
        self.set_path(&mut path.split('/').peekable(), data)
    }

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element with '/' as the separator.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an `Error`.
    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error> {
        self.get_path(&mut path.split('/').peekable(), data)
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// It will return all paths, even those that may be absent at run-time.
    ///
    /// # Template Arguments
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `TS` - The maximum length of the path in bytes.
    ///
    /// # Returns
    /// An `MiniconfIter` of paths or an `IterError` if `L` or `TS` are insufficient.
    fn iter_paths<const L: usize, const TS: usize>(
    ) -> Result<iter::MiniconfIter<Self, L, TS>, IterError> {
        let meta = Self::metadata();

        if TS < meta.max_length {
            return Err(IterError::PathLength);
        }

        if L < meta.max_depth {
            return Err(IterError::PathDepth);
        }

        Ok(Self::unchecked_iter_paths(Some(meta.count)))
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// It will return all paths, even those that may be absent at run-time.
    ///
    /// # Note
    /// This does not check that the path size or state vector are large enough. If they are not,
    /// panics may be generated internally by the library.
    ///
    /// # Args
    /// * `count`: Optional iterator length if known.
    ///
    /// # Template Arguments
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `TS` - The maximum length of the path in bytes.
    fn unchecked_iter_paths<const L: usize, const TS: usize>(
        count: core::option::Option<usize>,
    ) -> iter::MiniconfIter<Self, L, TS> {
        iter::MiniconfIter::new(count)
    }

    /// Deserialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: A `Peekable` `Iterator` identifying the element.
    /// * `value`: A slice containing the data to be deserialized.
    ///
    /// # Returns
    /// The number of bytes consumed from `value` or an `Error`.
    fn set_path<'a, P: Peekable<Item = &'a str>>(
        &mut self,
        path_parts: &'a mut P,
        value: &[u8],
    ) -> Result<usize, Error>;

    /// Serialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: A `Peekable` `Iterator` identifying the element.
    /// * `value`: A slice for the value to be serialized into.
    ///
    /// # Returns
    /// The number of bytes written to `value` or an `Error`.
    fn get_path<'a, P: Peekable<Item = &'a str>>(
        &self,
        path_parts: &'a mut P,
        value: &mut [u8],
    ) -> Result<usize, Error>;

    /// Get the next path in the namespace.
    ///
    /// # Args
    /// * `state`: A state array indicating the path to be retrieved.
    ///   A zeroed vector indicates the first path. The vector is advanced
    ///   such that the next element will be retrieved when called again.
    ///   The array needs to be at least as long as the maximum path depth.
    /// * `path`: A string to write the path into.
    ///
    /// # Returns
    /// A `bool` indicating a valid path was written to `path` from the given `state`.
    /// If `false`, `path` is invalid and there are no more paths within `self` at and
    /// beyond `state`.
    /// May return `IterError` indicating insufficient `state` or `path` size.
    fn next_path<const TS: usize>(
        state: &mut [usize],
        path: &mut heapless::String<TS>,
    ) -> Result<bool, IterError>;

    /// Get metadata about the paths in the namespace.
    fn metadata() -> Metadata;
}
