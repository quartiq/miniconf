#![no_std]
#![doc = include_str!("../README.md")]

mod array;
mod iter;
mod option;

pub use array::Array;
pub use iter::{IterError, MiniconfIter};
pub use miniconf_derive::Miniconf;
pub use option::Option;

#[cfg(feature = "mqtt-client")]
mod mqtt_client;

#[cfg(feature = "mqtt-client")]
pub use mqtt_client::MqttClient;

// Re-exports
pub use heapless;
pub use serde;
pub use serde_json_core;

#[cfg(feature = "mqtt-client")]
pub use minimq;

#[cfg(feature = "mqtt-client")]
pub use minimq::embedded_time;

#[doc(hidden)]
pub use serde::{
    de::{Deserialize, DeserializeOwned},
    ser::Serialize,
};

/// Errors that can occur when using the [Miniconf] API.
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
    /// Check that the serialized data is valid and of the correct type.
    Deserialization,

    /// The value provided could not be serialized.
    ///
    /// Check that the buffer had sufficient space.
    Serialization,

    /// When indexing into an array, the index provided was out of bounds.
    ///
    /// Check array indices to ensure that bounds for all paths are respected.
    BadIndex,

    /// The path does not exist at runtime.
    ///
    /// This is the case if a deferred [core::option::Option] or [Option]
    /// is `None` at runtime. `PathAbsent` takes precedence over `PathNotFound`
    /// if the path is simultaneously masked by a `Option::None` at runtime but
    /// would still be non-existent if it weren't.
    PathAbsent,
}

impl From<Error> for u8 {
    fn from(err: Error) -> u8 {
        match err {
            Error::PathNotFound => 1,
            Error::PathTooLong => 2,
            Error::PathTooShort => 3,
            Error::Deserialization => 5,
            Error::BadIndex => 6,
            Error::Serialization => 7,
            Error::PathAbsent => 8,
        }
    }
}

/// Metadata about a [Miniconf] namespace.
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

/// Helper trait for [core::iter::Peekable].
pub trait Peekable: core::iter::Iterator {
    fn peek(&mut self) -> core::option::Option<&Self::Item>;
}

impl<I: core::iter::Iterator> Peekable for core::iter::Peekable<I> {
    fn peek(&mut self) -> core::option::Option<&Self::Item> {
        core::iter::Peekable::peek(self)
    }
}

/// Trait exposing serialization/deserialization of elements by path.
pub trait Miniconf {
    /// Deserialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: A [Peekable] [Iterator] identifying the element.
    /// * `de`: A [serde::Deserializer] to use to deserialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), Error>
    where
        P: Peekable<Item = &'a str>,
        D: serde::Deserializer<'b>;

    /// Serialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: A [Peekable] [Iterator] identifying the element.
    /// * `ser`: A [serde::Serializer] to use to serialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error>
    where
        P: Peekable<Item = &'a str>,
        S: serde::Serializer;

    /// Get the next path in the namespace.
    ///
    /// This is usually not called directly but through a [MiniconfIter] returned by [SerDe::iter_paths].
    ///
    /// # Args
    /// * `state`: A state array indicating the path to be retrieved.
    ///   A zeroed vector indicates the first path. The vector is advanced
    ///   such that the next element will be retrieved when called again.
    ///   The array needs to be at least as long as the maximum path depth.
    /// * `path`: A string to write the path into.
    /// * `separator` - The path hierarchy separator.
    ///
    /// # Returns
    /// A `bool` indicating a valid path was written to `path` from the given `state`.
    /// If `false`, `path` is invalid and there are no more paths within `self` at and
    /// beyond `state`.
    /// May return `IterError` indicating insufficient `state` or `path` size.
    fn next_path<const TS: usize>(
        state: &mut [usize],
        path: &mut heapless::String<TS>,
        separator: char,
    ) -> Result<bool, IterError>;

    /// Get metadata about the paths in the namespace.
    fn metadata() -> Metadata;
}

pub trait SerDe<S>: Miniconf {
    /// The path hierarchy separator.
    ///
    /// This is passed to [Miniconf::next_path] by [MiniconfIter] and
    /// used in [SerDe::set] and [SerDe::get] to split the path.
    const SEPARATOR: char;

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// The iterator will walk all paths, even those that may be absent at run-time (see [Option]).
    /// The iterator has an exact and trusted [Iterator::size_hint].
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `TS` - The maximum length of the path in bytes.
    ///
    /// # Returns
    /// A [MiniconfIter] of paths or an [IterError] if `L` or `TS` are insufficient.
    fn iter_paths<const L: usize, const TS: usize>(
    ) -> Result<iter::MiniconfIter<Self, L, TS, S>, IterError> {
        iter::MiniconfIter::new()
    }

    /// Create an unchecked iterator of all possible paths.
    ///
    /// See also [SerDe::iter_paths].
    ///
    /// # Panic
    /// This does not check that the path size or state vector are large enough.
    /// While this function will not panic itself, calling `Iterator::next()` on its
    /// return value may.
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `TS` - The maximum length of the path in bytes.
    ///
    /// # Returns
    /// A [MiniconfIter] of paths or an [IterError] if `L` or `TS` are insufficient.
    fn unchecked_iter_paths<const L: usize, const TS: usize>() -> iter::MiniconfIter<Self, L, TS, S>
    {
        iter::MiniconfIter::default()
    }

    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error>;
}

/// Marker struct for [SerDe].
///
/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization payload format.
pub struct JsonCoreSlash;

impl<T> SerDe<JsonCoreSlash> for T
where
    T: Miniconf,
{
    const SEPARATOR: char = '/';

    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error> {
        let mut de = serde_json_core::de::Deserializer::new(data);
        self.set_path(&mut path.split(Self::SEPARATOR).peekable(), &mut de)?;
        de.end().map_err(|_| Error::Deserialization)
    }

    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error> {
        let mut ser = serde_json_core::ser::Serializer::new(data);
        self.get_path(&mut path.split(Self::SEPARATOR).peekable(), &mut ser)?;
        Ok(ser.end())
    }
}
