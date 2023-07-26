#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(feature = "json-core", doc = include_str!("../README.md"))]

use core::fmt::Write;

pub use miniconf_derive::Miniconf;
mod array;
pub use array::*;
mod iter;
pub use iter::*;
mod option;
pub use option::*;

#[cfg(feature = "json-core")]
mod json_core;
#[cfg(feature = "json-core")]
pub use json_core::*;

#[cfg(feature = "json")]
mod json;
#[cfg(feature = "json")]
pub use json::*;

#[cfg(feature = "mqtt-client")]
pub use minimq; // re-export
#[cfg(feature = "mqtt-client")]
mod mqtt_client;
#[cfg(feature = "mqtt-client")]
pub use mqtt_client::*;

pub use serde; // re-export
#[doc(hidden)]
pub use serde::{de::DeserializeOwned, Serialize};

/// Errors that occur during iteration over topic paths.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IterError {
    /// No element was found at the given depth
    Next(usize),

    /// The provided state vector is not long enough.
    Depth,

    /// The provided buffer is not long enough.
    Length,
}

/// Errors that can occur when using the [Miniconf] API.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error<T> {
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

    /// The value provided could not be serialized or deserialized.
    ///
    /// Check that the serialized data is valid and of the correct type.
    /// Check that the buffer had sufficient space.
    SerDe(T),

    /// There was an error after deserializing a value.
    ///
    /// If the `Deserializer` encounters an error only after successfully
    /// deserializing a value (as is the case if there is additional unexpected data),
    /// the update may have taken place but this error will still be returned.
    PostDeserialization(T),

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

impl<T> From<T> for Error<T> {
    fn from(err: T) -> Self {
        // By default in our context every T is a SerDe error.
        Error::SerDe(err)
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

/// Trait exposing serialization/deserialization of elements by path.
pub trait Miniconf {
    /// Deserialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: An [Iterator] identifying the element.
    /// * `de`: A [serde::Deserializer] to use to deserialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn set_path<'a, 'b: 'a, P, D>(
        &mut self,
        path_parts: &mut P,
        de: D,
    ) -> Result<(), Error<D::Error>>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>;

    /// Serialize an element by path.
    ///
    /// # Args
    /// * `path_parts`: An [Iterator] identifying the element.
    /// * `ser`: A [serde::Serializer] to use to serialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer;

    /// Get the next path in the namespace.
    ///
    /// This is usually not called directly but through a [MiniconfIter] returned by [SerDe::iter_paths].
    ///
    /// # Args
    /// * `state`: A state slice indicating the path to be retrieved.
    ///   A zeroed vector indicates the first path.
    ///   The slice needs to be at least as long as the maximum path depth.
    /// * `depth`: The path depth this struct is at.
    /// * `path`: A string to write the path into.
    /// * `separator` - The path hierarchy separator.
    ///
    /// # Returns
    /// A `usize` indicating the final depth of the valid path.
    /// Must return `IterError::Next(usize)` with the final depth if there are
    /// no more elements at that index and depth.
    /// May return `IterError` indicating insufficient `state` or `path` size.
    fn next_path(
        state: &[usize],
        depth: usize,
        path: impl Write,
        separator: char,
    ) -> Result<usize, IterError>;

    /// Get metadata about the paths in the namespace.
    ///
    /// # Args
    /// * `separator_length` - The path hierarchy separator length in bytes.
    fn metadata(separator_length: usize) -> Metadata;
}

/// Trait for implementing a specific way of serialization/deserialization into/from a slice
/// and splitting/joining the path with a separator.
pub trait SerDe<S>: Miniconf + Sized {
    /// The path hierarchy separator.
    ///
    /// This is passed to [Miniconf::next_path] by [MiniconfIter] and
    /// used in [SerDe::set] and [SerDe::get] to split the path.
    const SEPARATOR: char;

    /// The [serde::Serializer::Error] type.
    type SerError;
    /// The [serde::Deserializer::Error] type.
    type DeError;

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// The iterator will walk all paths, even those that may be absent at run-time (see [Option]).
    /// The iterator has an exact and trusted [Iterator::size_hint].
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `P`  - The type to hold the path.
    ///
    /// # Returns
    /// A [MiniconfIter] of paths or an [IterError] if `L` is insufficient.
    fn iter_paths<const L: usize, P>() -> Result<iter::MiniconfIter<Self, S, L, P>, IterError> {
        MiniconfIter::new()
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
    /// * `P`  - The type to hold the path.
    ///
    /// # Returns
    /// A [MiniconfIter] of paths or an [IterError] if `L` is insufficient.
    fn unchecked_iter_paths<const L: usize, P>() -> MiniconfIter<Self, S, L, P> {
        MiniconfIter::default()
    }

    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error<Self::DeError>>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<Self::SerError>>;
}
