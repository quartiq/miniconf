#![no_std]
#![doc = include_str!("../README.md")]

mod array;
mod iter;
mod option;

pub use array::Array;
pub use iter::MiniconfIter;
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
    /// * `path_parts`: A `Peekable` `Iterator` identifying the element.
    /// * `value`: A slice containing the data to be deserialized.
    ///
    /// # Returns
    /// The number of bytes consumed from `value` or an `Error`.
    fn set_path<'a, 'b: 'a, P, D>(
        &mut self,
        path_parts: &mut P,
        de: &'a mut D,
    ) -> Result<(), Error>
    where
        P: Peekable<Item = &'a str>,
        &'a mut D: serde::Deserializer<'b>;

    /// Serialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: A `Peekable` `Iterator` identifying the element.
    /// * `value`: A slice for the value to be serialized into.
    ///
    /// # Returns
    /// The number of bytes written to `value` or an `Error`.
    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: &'a mut S) -> Result<(), Error>
    where
        P: Peekable<Item = &'a str>,
        &'a mut S: serde::Serializer;

    /// Get the next path in the namespace.
    ///
    /// This is usually not called directly but through a [MiniconfIter] returned by [Miniconf::iter_paths].
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
    /// # Args
    /// * `separator` - The path hierarchy separator.
    ///
    /// # Returns
    /// A [MiniconfIter] of paths or an [IterError] if `L` or `TS` are insufficient.
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
        iter::MiniconfIter::new(count, Self::SEPARATOR)
    }

    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element with '/' as the separator.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element with '/' as the separator.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error>;
}

pub struct JsonCoreSlash;

/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization format.
impl<T> SerDe<JsonCoreSlash> for T
where
    T: Miniconf,
{
    const SEPARATOR: char = '/';

    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error> {
        let mut de = serde_json_core::de::Deserializer::new(data);
        self.set_path(
            &mut path
                .split(<Self as SerDe<JsonCoreSlash>>::SEPARATOR)
                .peekable(),
            &mut de,
        )?;
        de.end().map_err(|_| Error::Deserialization)
    }

    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error> {
        let mut ser = serde_json_core::ser::Serializer::new(data);
        self.get_path(
            &mut path
                .split(<Self as SerDe<JsonCoreSlash>>::SEPARATOR)
                .peekable(),
            &mut ser,
        )?;
        Ok(ser.end())
    }
}
