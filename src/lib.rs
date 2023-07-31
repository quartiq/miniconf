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
/// `Absent > TooShort > NotFound > TooLong > Inner > PostDeserialization`
/// before any `Ok`.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error<E> {
    /// The key is valid, but does not exist at runtime.
    ///
    /// This is the case if a deferred [core::option::Option] or [Option]
    /// is `None` at runtime.
    Absent(usize),

    /// The key ends early and does not reach a leaf node.
    TooShort(usize),

    /// A key was not found (index unparsable or too large, name not fod or invalid).
    NotFound(usize),

    /// The key is too long and goes beyond a leaf node.
    TooLong(usize),

    /// The value provided could not be serialized or deserialized
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

/// Struct to indicate a short indices slice or a too small iterator state.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SliceShort;

/// Pass the [`Result`] up one hierarchy level.
pub trait Increment {
    /// Increment the `depth` member by one.
    fn increment(self) -> Self;
}

impl<E> Increment for Result<usize, Error<E>> {
    fn increment(self) -> Self {
        match self {
            Ok(i) => Ok(i + 1),
            Err(Error::NotFound(i)) => Err(Error::NotFound(i + 1)),
            Err(Error::TooShort(i)) => Err(Error::TooShort(i + 1)),
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

/// Capability to convert a key into an node index.
pub trait Key {
    /// Convert the key `self` to a `usize` index.
    fn find<const D: usize, M: Miniconf<D>>(&self) -> core::option::Option<usize>;
}

impl Key for usize {
    #[inline]
    fn find<const D: usize, M>(&self) -> core::option::Option<usize> {
        Some(*self)
    }
}

impl Key for &str {
    #[inline]
    fn find<const D: usize, M: Miniconf<D>>(&self) -> core::option::Option<usize> {
        M::name_to_index(self)
    }
}

/// Trait exposing serialization/deserialization of nodes by keys/paths and traversal.
///
/// The parameter `Y` is a lower bound for the recursion depth. It's only non-unity in
/// `array` and `option` as they need a configurable recursion depth at compile time
/// and can't be marked up with inner attributes.
/// The paramter `Y` deviates from the key tree depth significantly:
/// * `Option` consume a layer of Miniconf recursion, but don't add to key tree height
/// * A struct that has `Miniconf` derived for it will only implement `Miniconf<1>`
///   but may perform arbitrarily recursion and also have arbitrarily long keys.
///   `1` is then the minimum recursion depth for a (non-newtype) struct. No other
///   value is needed or valid.
pub trait Miniconf<const Y: usize> {
    /// Convert a node name to a node index.
    fn name_to_index(name: &str) -> core::option::Option<usize>;

    /// Serialize a node by keys.
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `ser`: A `Serializer` to to serialize the value.
    ///
    /// # Returns
    /// Node depth on success.
    fn get_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Iterator,
        K::Item: Key,
        S: serde::Serializer;

    /// Deserialize an node by keys.
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `de`: A `Deserializer` to deserialize the value.
    ///
    /// # Returns
    /// Node depth on success
    fn set_by_key<'a, K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        K::Item: Key,
        D: serde::Deserializer<'a>;

    /// Call a function for each node on the path described by keys.
    ///
    /// Traversal is aborted once `func` returns an `Err(E)`.
    ///
    /// May not exhaust `keys` if a leaf is found early. i.e. `keys`
    /// may be longer than required.
    /// If `Self` is a leaf, nothing will be consumed from `keys`
    /// and `Ok(0)` will be returned.
    /// If `Self` is non-leaf (internal node) and the iterator is
    /// exhausted (empty),
    /// `Err(Error::TooShort(0))` will be returned.
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `func`: A `FnMut` to be called for each node on the path. Its arguments are
    ///   (a) the index of the node at the given depth,
    ///   (b) the name of the node at the given depth.
    ///
    /// # Returns
    /// Final node depth on success
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Iterator,
        K::Item: Key,
        // Writing this to return an iterator instead of using a callback
        // would have worse performance (O(n^2) instead of O(n))
        F: FnMut(usize, &str) -> Result<(), E>;

    /// Get metadata about the paths in the namespace.
    fn metadata() -> Metadata;

    /// Convert keys to path.
    ///
    /// This is typically called through a [PathIter] returned by [Miniconf::iter_paths].
    ///
    /// `keys` may be longer than required. Extra items are ignored.
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `path`: A string to write the separators and node names into.
    ///   See also [Miniconf::metadata()] for upper bounds on path length.
    /// * `sep`: The path hierarchy separator to be inserted before each name.
    ///
    /// # Returns
    /// Final node depth on success
    fn path<K, P>(keys: K, mut path: P, sep: &str) -> Result<usize, Error<core::fmt::Error>>
    where
        K: IntoIterator,
        K::Item: Key,
        P: core::fmt::Write,
    {
        Self::traverse_by_key(keys.into_iter(), |_index, name| {
            path.write_str(sep).and_then(|_| path.write_str(name))
        })
    }

    /// Convert keys to `indices`.
    ///
    /// This determines the `indices` of the item specified by `keys`.
    ///
    /// See also [`Miniconf::path()`] for the analogous function.
    ///
    /// Entries in `indices` at and beyond the `depth` returned are unaffected.
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `indices`: An iterator of `&mut usize` to write the node indices into.
    ///   If `indices` is shorter than the node depth, `Error<SliceShort>` is returned
    ///   See also [Miniconf::metadata()] for upper bounds on depth.
    ///
    /// # Returns
    /// Final node depth on success
    fn indices<'a, K, I>(keys: K, indices: I) -> Result<usize, Error<SliceShort>>
    where
        K: IntoIterator,
        K::Item: Key,
        I: IntoIterator<Item = &'a mut usize>,
    {
        let mut indices = indices.into_iter();
        Self::traverse_by_key(keys.into_iter(), |index, _name| {
            let idx = indices.next().ok_or(SliceShort)?;
            *idx = index;
            Ok(())
        })
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// The iterator will walk all paths, including those that may be absent at
    /// run-time (see [Option]).
    /// The iterator has an exact and trusted [Iterator::size_hint].
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. the number of separators.
    /// * `P`  - The type to hold the path. Needs to be `core::fmt::Write`.
    ///
    /// # Args
    ///
    /// # Returns
    /// An iterator of paths or an Error if `L` is insufficient.
    #[inline]
    fn iter_paths<const L: usize, P: core::fmt::Write>(
        separator: &str,
    ) -> Result<PathIter<'_, Self, L, P, Y>, SliceShort> {
        PathIter::new(separator)
    }

    /// Create an unchecked iterator of all possible paths.
    ///
    /// See also [Miniconf::iter_paths].
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. the number of separators.
    /// * `P`  - The type to hold the path. Needs to be `core::fmt::Write`.
    ///
    /// # Returns
    /// A iterator of paths.
    #[inline]
    fn iter_paths_unchecked<const L: usize, P: core::fmt::Write>(
        separator: &str,
    ) -> PathIter<'_, Self, L, P, Y> {
        PathIter::new_unchecked(separator)
    }
}
