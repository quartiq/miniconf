#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(feature = "json-core", doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "json-core"), doc = "Miniconf")]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![deny(missing_docs)]
#![deny(warnings)]
#![deny(unsafe_code)]

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
/// A `usize` member indicates the depth where the error occurred.
/// The depth is the number of names or indices consumed.
/// It is also the number of separators in a path or the length
/// of an indices slice.
///
/// The precedence in the case where multiple errors are applicable
/// simultaneously is from high to low:
///
/// `Internal > Absent > TooLong > NotFound > Inner > PostDeserialization`
/// before any `Ok`.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error<E> {
    /// A name was not found or an index was too large or invalid.
    NotFound(usize),

    /// The names/indices are valid, but do not exist at runtime.
    ///
    /// This is the case if a deferred [core::option::Option] or [Option]
    /// is `None` at runtime.
    Absent(usize),

    /// The names/indices are valid, but there is trailing data at the end.
    TooLong(usize),

    /// The names/indices are valid, but end early and do not reach a leaf.
    Internal(usize),

    /// The value provided could not be serialized or deserialized or
    /// or the traversal function returned an error.
    Inner(E),

    /// There was an error after deserializing a value.
    ///
    /// If the `Deserializer` encounters an error only after successfully
    /// deserializing a value (as is the case if there is additional unexpected data),
    /// the update takes place but this error will still be returned.
    PostDeserialization(E),
}

impl<T> From<T> for Error<T> {
    fn from(value: T) -> Self {
        Error::Inner(value)
    }
}

/// `Ok` return types for the [Miniconf] API.
/// A `usize` member indicates the depth where the traversal ended.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Ok {
    /// Non-leaf at depth
    Internal(usize),
    /// Leaf at depth
    Leaf(usize),
}

/// Shorthand type alias.
pub type Result<E> = core::result::Result<Ok, Error<E>>;

/// Struct to indicate a short indices slice or a too small iterator state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SliceShort;

/// Pass the [`Result`] up one hierarchy level.
pub trait Increment {
    /// Increment the `depth` member by one.
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
    /// This does not include separators.
    pub max_length: usize,

    /// The maximum path depth.
    pub max_depth: usize,

    /// The number of paths.
    pub count: usize,
}

impl Metadata {
    /// To obtain an upper bound on the maximum length of all paths
    /// including separators, add `max_depth*separator_length`.
    pub fn separator(self, separator: &str) -> Self {
        Self {
            max_length: self.max_length + self.max_depth * separator.len(),
            ..self
        }
    }
}

/// Capability to convert a key into an element index.
pub trait Key {
    /// Convert the key `self` to a `usize` index.
    fn find<M: Miniconf>(self) -> core::option::Option<usize>;
}

impl Key for usize {
    fn find<M: Miniconf>(self) -> core::option::Option<usize> {
        Some(self)
    }
}

impl Key for &str {
    fn find<M: Miniconf>(self) -> core::option::Option<usize> {
        M::name_to_index(self)
    }
}

/// Trait exposing serialization/deserialization of elements by path and traversal by path/indices.
pub trait Miniconf {
    /// Convert a name key to an index.
    fn name_to_index(value: &str) -> core::option::Option<usize>;

    /// Deserialize an element by key.
    ///
    /// # Args
    /// * `keys`: An `Iterator` identifying the element. The iterator items
    ///    must support conversion to graph indices through [`Key`]
    /// * `de`: A `Deserializer` to deserialize the value.
    ///
    /// # Returns
    /// [`Ok`] on success, [Error] on failure.
    fn set_by_key<'a, P, D>(&mut self, keys: P, de: D) -> Result<D::Error>
    where
        P: Iterator,
        D: serde::Deserializer<'a>,
        P::Item: Key;

    /// Serialize an element by key.
    ///
    /// # Args
    /// * `keys`: An `Iterator` identifying the element. The iterator items
    ///    must support conversion to graph indices through [`Key`]
    /// * `ser`: A `Serializer` to to serialize the value.
    ///
    /// # Returns
    /// [`Ok`] on success, [Error] on failure.
    fn get_by_key<P, S>(&self, keys: P, ser: S) -> Result<S::Error>
    where
        P: Iterator,
        S: serde::Serializer,
        P::Item: Key;

    /// Call `func` for each element on the path described by a key.
    ///
    /// Traversal is aborted once `func` returns an `Err(E)`.
    ///
    /// May not exhaust the iterator if a leaf is found early. i.e. keys may be too long.
    /// If `Self` is a leaf, nothing will be consumed from the iterator
    /// and [`Ok::Leaf(0)`] will be returned.
    /// If `Self` is non-leaf (internal) and the iterator is exhausted (empty),
    /// [`Ok::Internal(0)`] will be returned.
    ///
    /// # Args
    /// * `keys`: An iterator identifying the element.
    /// * `func`: A `FnMut` to be called for each element on the path. Its arguments are
    ///    (a) an [Ok] indicating whether this is an internal or leaf node,
    ///    (b) the index of the element at the given depth,
    ///    (c) the name of the element at the given depth.
    fn traverse_by_key<P, F, E>(keys: P, func: F) -> Result<E>
    where
        P: Iterator,
        P::Item: Key,
        F: FnMut(Ok, usize, &str) -> core::result::Result<(), E>;

    /// Get metadata about the paths in the namespace.
    fn metadata() -> Metadata;

    /// Convert indices to path.
    ///
    /// This is usually not called directly but through a [PathIter] returned by [Miniconf::iter_paths].
    ///
    /// # Args
    /// * `keys`: A key iterator indicating the path to be retrieved.
    ///   An empty iterator indicates the root.
    ///   An iterator yielding zeros indicates the first path.
    /// * `path`: A string to write the path into.
    /// * `sep`: The path hierarchy separator. It is inserted before each name.
    ///
    /// # Returns
    /// A [Ok] where the `usize` member indicates the final depth of the valid path.
    /// A [Error] if there was an error.
    fn path<I, N>(keys: I, path: &mut N, sep: &str) -> Result<core::fmt::Error>
    where
        I: Iterator,
        I::Item: Key,
        N: core::fmt::Write,
    {
        Self::traverse_by_key(keys, |ok, _index, name| {
            if ok == Ok::Leaf(0) {
                Ok(())
            } else {
                path.write_str(sep).and_then(|_| path.write_str(name))
            }
        })
    }

    /// Convert `path` to `indices`.
    ///
    /// This determines the `indices` of the item specified by `path`.
    ///
    /// See also [`Miniconf::path()`] for the analogous function.
    ///
    /// Entries in `indices` at and beyond the `depth` returned are unaffected.
    ///
    /// # Args
    /// * `keys`: An key iterator of path elements.
    /// * `indices`: A slice to write the element indices into.
    ///   The slice needs to be at least as long as the maximum path depth ([Metadata]).
    ///
    /// # Returns
    /// A [Ok] where the `usize` member indicates the final depth of the valid path.
    /// A [Error] println!("{}", if there was an error.is_some());
    fn indices<P>(keys: P, indices: &mut [usize]) -> Result<SliceShort>
    where
        P: Iterator,
        P::Item: Key,
    {
        let mut depth = 0;
        Self::traverse_by_key(keys, |ok, index, _name| {
            if ok == Ok::Leaf(0) {
                Ok(())
            } else if indices.len() < depth {
                Err(SliceShort)
            } else {
                indices[depth] = index;
                depth += 1;
                Ok(())
            }
        })
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// The iterator will walk all paths, even those that may be absent at run-time (see [Option]).
    /// The iterator has an exact and trusted [Iterator::size_hint].
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. the number of separators.
    /// * `P`  - The type to hold the path.
    ///
    /// # Args
    ///
    /// # Returns
    /// An iterator of paths or an [Error] if `L` is insufficient.
    fn iter_paths<const L: usize, P>(
        separator: &str,
    ) -> core::result::Result<PathIter<'_, Self, L, P>, Error<SliceShort>> {
        PathIter::new(separator)
    }

    /// Create an unchecked iterator of all possible paths.
    ///
    /// See also [Miniconf::iter_paths].
    ///
    /// # Panic
    /// `L` must be sufficiently large to hold the iterator state.
    /// While this function will not panic itself, calling `Iterator::next()` on its
    /// return value may.
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `P`  - The type to hold the path.
    ///
    /// # Returns
    /// A iterator of paths.
    fn unchecked_iter_paths<const L: usize, P>(separator: &str) -> PathIter<'_, Self, L, P> {
        PathIter::new_unchecked(separator)
    }
}
