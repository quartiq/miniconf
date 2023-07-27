#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(feature = "json-core", doc = include_str!("../README.md"))]

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

/// Errors that can occur when using the [Miniconf] API.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error<E> {
    /// The provided path wasn't found in the structure.
    ///
    /// Double check the provided path to verify that it's valid.
    ///
    /// Index entry too large at depth or invalid name.
    NotFound(usize),

    /// The provided path was valid, but there was trailing data at the end.
    ///
    /// Check the end of the path and remove any excess characters.
    TooLong(usize),

    /// The provided path was valid, but did not specify a value fully.
    ///
    /// Double check the ending and add the remainder of the path.
    Internal(usize),

    /// The path does not exist at runtime.
    ///
    /// This is the case if a deferred [core::option::Option] or [Option]
    /// is `None` at runtime. `PathAbsent` takes precedence over `PathNotFound`
    /// if the path is simultaneously masked by a `Option::None` at runtime but
    /// would still be non-existent if it weren't.
    Absent(usize),

    /// The value provided could not be serialized or deserialized.
    ///
    /// Check that the serialized data is valid and of the correct type.
    /// Check that the buffer had sufficient space.
    /// Inner error, e.g.
    /// Formating error (Write::write_str failure, for `name()`)
    /// or
    /// Index too short (for `index()`)
    Inner(E),

    /// There was an error after deserializing a value.
    ///
    /// If the `Deserializer` encounters an error only after successfully
    /// deserializing a value (as is the case if there is additional unexpected data),
    /// the update may have taken place but this error will still be returned.
    PostDeserialization(E),
}

impl<T> From<T> for Error<T> {
    fn from(value: T) -> Self {
        Error::Inner(value)
    }
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Ok {
    /// Non-leaf at depth
    Internal(usize),
    /// Leaf at depth
    Leaf(usize),
}

pub type Result<E> = core::result::Result<Ok, Error<E>>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SliceShort;

pub trait Increment {
    fn increment(self) -> Self;
}

impl<E> Increment for Result<E> {
    fn increment(self) -> Self {
        match self {
            Ok(Ok::Internal(i)) => Ok(Ok::Internal(i + 1)),
            Ok(Ok::Leaf(i)) => Ok(Ok::Leaf(i + 1)),
            Err(Error::NotFound(i)) => Err(Error::NotFound(i + 1)),
            Err(Error::Internal(i)) => Err(Error::Internal(i + 1)),
            Err(Error::TooLong(i)) => Err(Error::TooLong(i + 1)),
            Err(Error::Absent(i)) => Err(Error::Absent(i + 1)),
            e => e,
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
    fn set_by_name<'a, 'b: 'a, P, D>(&mut self, names: &mut P, de: D) -> Result<D::Error>
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
    fn get_by_name<'a, P, S>(&self, names: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer;

    fn traverse_by_name<'a, P, F, E>(names: &mut P, func: F, internal: bool) -> Result<E>
    where
        P: Iterator<Item = &'a str>,
        F: FnMut(usize, &str) -> core::result::Result<(), E>;

    fn traverse_by_index<P, F, E>(indices: &mut P, func: F, internal: bool) -> Result<E>
    where
        P: Iterator<Item = usize>,
        F: FnMut(usize, &str) -> core::result::Result<(), E>;

    /// Get metadata about the paths in the namespace.
    ///
    /// # Args
    /// * `separator_length` - The path hierarchy separator length in bytes.
    fn metadata(separator_length: usize) -> Metadata;

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
    ///
    /// Write the `name` of the item specified by `index`.
    /// May not exhaust the iterator if a Leaf is found early. I.e. the index may be too long.
    /// If `Self` is a leaf, nothing will be consumed from `index` or
    /// written to `name` and `Leaf(0)` will be returned.
    /// If `Self` is non-leaf and  `index` is exhausted, nothing will be written to `name` and
    /// `Internal(0)` will be returned.
    /// If `full`, all path elements are written, otherwise only the final element.
    /// Each element written will always be prefixed by the separator.
    fn path<I, N>(indices: &mut I, path: &mut N, sep: &str) -> Result<core::fmt::Error>
    where
        I: Iterator<Item = usize>,
        N: core::fmt::Write,
    {
        Self::traverse_by_index(
            indices,
            |_index, name| {
                path.write_str(sep).and_then(|_| path.write_str(name))?;
                Ok(())
            },
            true,
        )
    }

    /// Determine the `index` of the item specified by `path`.
    /// May not exhaust the iterator if leaf is found early. I.e. the path may be too long.
    /// If `Self` is a leaf, nothing will be consumed from `path` or
    /// written to `index` and `Leaf(0)` will be returned.
    /// If `Self` is non-leaf and  `path` is exhausted, nothing will be written to `index` and
    /// `Internal(0)` will be returned.
    /// Entries in `index` at and beyond the `depth` returned are unaffected.
    fn indices<'a, P>(path: &mut P, indices: &mut [usize]) -> Result<SliceShort>
    where
        P: Iterator<Item = &'a str>,
    {
        let mut depth = 0;
        Self::traverse_by_name(
            path,
            |index, _name| {
                if indices.len() < depth {
                    Err(SliceShort)
                } else {
                    indices[depth] = index;
                    depth += 1;
                    Ok(())
                }
            },
            true,
        )
    }
}

/// Trait for implementing a specific way of serialization/deserialization into/from a slice
/// and splitting/joining the path with a separator.
pub trait SerDe<S>: Miniconf + Sized {
    /// The path hierarchy separator.
    ///
    /// This is passed to [Miniconf::next_path] by [MiniconfIter] and
    /// used in [SerDe::set] and [SerDe::get] to split the path.
    const SEPARATOR: &'static str;

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
    fn iter_paths<const L: usize, P>(
    ) -> core::result::Result<iter::MiniconfIter<Self, S, L, P>, Error<SliceShort>> {
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
    fn set(&mut self, path: &str, data: &[u8])
        -> core::result::Result<usize, Error<Self::DeError>>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get(
        &self,
        path: &str,
        data: &mut [u8],
    ) -> core::result::Result<usize, Error<Self::SerError>>;
}
