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

pub trait Increment {
    /// Pass the `Result` up one hierarchy level.
    /// This increments the `depth` member.
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

/// Trait exposing serialization/deserialization of elements by path and traversal by path/indices.
pub trait Miniconf {
    /// Deserialize an element by path.
    ///
    /// # Args
    /// * `names`: An [Iterator] identifying the element.
    /// * `de`: A [serde::Deserializer] to use to deserialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn set_by_name<'a, 'b, P, D>(&mut self, names: &mut P, de: D) -> Result<D::Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>;

    /// Serialize an element by path.
    ///
    /// # Args
    /// * `names`: An [Iterator] identifying the element.
    /// * `ser`: A [serde::Serializer] to use to serialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn get_by_name<'a, P, S>(&self, names: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer;

    /// Deserialize an element by index.
    ///
    /// # Args
    /// * `indices`: An [Iterator] identifying the element.
    /// * `de`: A [serde::Deserializer] to use to deserialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn set_by_index<'b, P, D>(&mut self, indices: &mut P, de: D) -> Result<D::Error>
    where
        P: Iterator<Item = usize>,
        D: serde::Deserializer<'b>;

    /// Serialize an element by index.
    ///
    /// # Args
    /// * `indices`: An [Iterator] identifying the element.
    /// * `ser`: A [serde::Serializer] to use to serialize the value.
    ///
    /// # Returns
    /// May return an [Error].
    fn get_by_index<P, S>(&self, indices: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator<Item = usize>,
        S: serde::Serializer;

    /// Call `func` for each element on a path.
    ///
    /// Traversal is aborted once `func` returns an `Err(E)`.
    ///
    /// # Args
    /// * `names`: An iterator identifying the element.
    /// * `func`: A `FnMut` to be called for each element on the path. Its arguments are
    ///    (a) an [Ok] indicating whether this is an internal or leaf node,
    ///    (b) the index of the element at the given depth,
    ///    (c) the name of the element at the given depth.
    fn traverse_by_name<'a, P, F, E>(names: &mut P, func: F) -> Result<E>
    where
        P: Iterator<Item = &'a str>,
        F: FnMut(Ok, usize, &str) -> core::result::Result<(), E>;

    /// Call `func` for each element on a path.
    ///
    /// Same as [`Miniconf::traverse_by_name()`] just for indices.
    fn traverse_by_index<P, F, E>(indices: &mut P, func: F) -> Result<E>
    where
        P: Iterator<Item = usize>,
        F: FnMut(Ok, usize, &str) -> core::result::Result<(), E>;

    /// Get metadata about the paths in the namespace.
    fn metadata() -> Metadata;

    /// Convert indices to path.
    ///
    /// This is usually not called directly but through a [Iter] returned by [Miniconf::iter_paths].
    ///
    /// May not exhaust the iterator if a Leaf is found early. I.e. the index may be too long.
    /// If `Self` is a leaf, nothing will be consumed from the iterator or
    /// written and [`Ok::Leaf(0)`] will be returned.
    /// If `Self` is non-leaf (internal) and the iterator is exhausted (empty),
    /// nothing will be written and [`Ok::Internal(0)`] will be returned.
    ///
    /// # Args
    /// * `indices`: A state slice indicating the path to be retrieved.
    ///   An empty vector indicates the root.
    ///   A zeroed vector indicates the first path.
    ///   The slice needs to be at least as long as the maximum path depth ([Metadata]).
    /// * `path`: A string to write the path into.
    /// * `sep`: The path hierarchy separator. It is inserted before each name.
    ///
    /// # Args
    /// * `indices`: A slice of indices describing the path.
    /// * `path`: The `Write` to write the path to.
    ///
    /// # Returns
    /// A [Ok] where the `usize` member indicates the final depth of the valid path.
    fn path<I, N>(indices: &mut I, path: &mut N, sep: &str) -> Result<core::fmt::Error>
    where
        I: Iterator<Item = usize>,
        N: core::fmt::Write,
    {
        Self::traverse_by_index(indices, |_ok, _index, name| {
            path.write_str(sep).and_then(|_| path.write_str(name))
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
    /// * `names`: An iterator of path elements.
    /// * `indices`: A slice to write the element indices into.
    ///
    /// # Returns
    /// A [Ok] where the `usize` member indicates the final depth of the valid path.
    fn indices<'a, P>(names: &mut P, indices: &mut [usize]) -> Result<SliceShort>
    where
        P: Iterator<Item = &'a str>,
    {
        let mut depth = 0;
        Self::traverse_by_name(names, |_ok, index, _name| {
            if indices.len() < depth {
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
    /// An [Iter] of paths or an [Error] if `L` is insufficient.
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
    /// This does not check that the path size or state vector are large enough.
    /// While this function will not panic itself, calling `Iterator::next()` on its
    /// return value may.
    ///
    /// # Generics
    /// * `L`  - The maximum depth of the path, i.e. number of separators plus 1.
    /// * `P`  - The type to hold the path.
    ///
    /// # Returns
    /// A [Iter] of paths.
    fn unchecked_iter_paths<const L: usize, P>(separator: &str) -> PathIter<'_, Self, L, P> {
        PathIter::new_unchecked(separator)
    }
}
