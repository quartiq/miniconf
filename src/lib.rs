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
/// The keys used to locate nodes can bei either iterators over `usize` or iterators
/// over `&str` names.
///
/// # Design
///
/// The const parameter `Y` is the miniconf recursion depth. It defaults to `1`.
///
/// An implementor of `Miniconf<Y>` may consume at most `Y` items from the
/// `keys` iterator argument in the recursive methods ([`Miniconf::serialize_by_key()`],
/// [`Miniconf::deserialize_by_key()`], [`Miniconf::traverse_by_key()`]). This includes
/// both the items consumed directly before recursing and those consumed indirectly
/// by recursing into inner types. In the same way it may call `func` in
/// [`Miniconf::traverse_by_key()`] at most `Y` times, again including those calls due
/// to recursion into inner `Miniconf` types.
///
/// This implies that if an implementor `T` of `Miniconf<Y>` contains and recurses into
/// an inner type using that type's `Miniconf<Z>` implementation, then `Z <= Y` must
/// hold and `T` may consume at most `Y - Z` items from the `keys` iterators and call
/// `func` at most `Y - Z` times.
///
/// The recursion depth `Y` is thus an upper bound of the maximum key length
/// (the depth/height of the tree).
///
/// # Derive macro
///
/// A derive macro to automatically implement the correct `Miniconf<Y>` on a struct `S` is available at
/// [`macro@Miniconf`].
///
/// Each field in the struct must either implement [`serde::Serialize`] `+` [`serde::de::DeserializeOwned`]
/// (and be supported by the intended [`serde::Serializer`]/[`serde::Deserializer`] backend)
/// or implement [Miniconf].
///
/// For each field, the Miniconf recursion depth is configured through the `#[miniconf(defer(Y))]` attribute,
/// with `Y = 1` being the implied default when using `#[miniconf(defer)]` and `Y = 0` invalid.
/// If the attribute is not present, the field is a leaf and accessed only through its
/// [`serde::Serialize`]/[`serde::Deserialize`] implementation.
/// With the attribute present the field is accessed through its [`Miniconf<Y>`] implementation with the given
/// recursion depth.
///
/// Homogeneous [core::array]s can be made accessible either
/// 1. as a single leaf in the tree like other serde-capable items, or
/// 2. by item through their numeric indices (with the attribute `#[miniconf(defer(1))]`), or
/// 3. exposing a sub-tree per item with `#[miniconf(defer(D))]` and `D >= 2`.
///
/// `Option` is used
/// 1. as a leaf like a standard `serde` Option, or
/// 2. with `#[miniconf(defer(1))]` to support a leaf value that may be absent (masked) at runtime.
/// 3. with `#[miniconf(defer(D))]` and `D >= 2` to support masking sub-trees at runtime.
///
/// ## Bounds on generics
///
/// The macro adds bounds to generic types of the struct it is acting on.
/// E.g. If a generic type parameter `T` of the struct `S<T>`is used as a type parameter to a
/// field type `a: F1<F2<T>>` the type `T` will be considered to reside at depth `X = 2` (as it is
/// within `F2` which is within `F1`) and the following bounds will applied:
///
/// * With the `#[miniconf]` attribute not present, `T` will receive bounds `Serialize + DeserializeOwned`.
/// * With `#[miniconf(defer(Y))]`, and `Y - X < 1` it will also receive bounds `Serialize + DeserializeOwned`.
/// * For `Y - X >= 1` it will receive the bound `T: Miniconf<Y - X>`.
///
/// This behavior is upheld by and compatible with all implementations in this crate. It is only violated
/// when deriving `Miniconf` for a struct that (a) forwards its own type parameters as type
/// parameters to its field types, (b) uses `Miniconf` on those fields, and (c) those field
/// types use their type parameters at other levels than `Miniconf<Y - 1>`. See the
/// `test_derive_macro_bound_failure` test in `tests/generics.rs`.
///
/// # Example
///
/// ```rust
/// use miniconf::Miniconf;
///
/// #[derive(Miniconf)]
/// struct Nested {
///     #[miniconf(defer)]
///     data: [u32; 2],
/// }
/// #[derive(Miniconf)]
/// struct Settings {
///     // Accessed with path `nested/data/0` or `nested/data/1`
///     #[miniconf(defer(2))]
///     nested: Nested,
///
///     // Accessed with path `external`
///     external: bool,
/// }
/// ```
pub trait Miniconf<const Y: usize = 1> {
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
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
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
    fn deserialize_by_key<'a, K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
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
    /// An iterator of paths with a trusted and exact `size_hint()`.
    #[inline]
    fn iter_paths<P: core::fmt::Write>(separator: &str) -> PathIter<'_, Self, Y, P> {
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
    fn iter_paths_unchecked<P: core::fmt::Write>(separator: &str) -> PathIter<'_, Self, Y, P> {
        PathIter::new_unchecked(separator)
    }
}
