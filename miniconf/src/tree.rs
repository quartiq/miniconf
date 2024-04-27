use core::any::Any;
use core::fmt::{Display, Formatter, Write};

use crate::{IndexIter, IntoKeys, Keys, Packed, PackedIter, PathIter};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Errors that can occur when using the Tree traits.
///
/// A `usize` member indicates the key depth where the error occurred.
/// The depth here is the number of names or indices consumed.
/// It is also the number of separators in a path or the length
/// of an indices slice.
///
/// If multiple errors are applicable simultaneously the precedence
/// is as per the order in the enum definition (from high to low).
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error<E> {
    /// The key is valid, but does not exist at runtime.
    ///
    /// This is the case if an [`Option`] using the `Tree*` traits
    /// is `None` at runtime. See also [`TreeKey#option`].
    Absent(usize),

    /// The key ends early and does not reach a leaf node.
    TooShort(usize),

    /// The key was not found (index unparsable or too large, name not found or invalid).
    NotFound(usize),

    /// The key is too long and goes beyond a leaf node.
    TooLong(usize),

    /// An field could not be accessed.
    ///
    /// The `get` or `get_mut` accessor returned an error message.
    Access(usize, &'static str),

    /// The value provided could not be serialized or deserialized
    /// or the traversal function returned an error.
    Inner(usize, E),

    /// There was an error during finalization.
    ///
    /// The `Deserializer` has encountered an error only after successfully
    /// deserializing a value. This is the case if there is additional unexpected data.
    /// The [`TreeDeserialize::deserialize_by_key()`] update takes place but this
    /// error will be returned.
    ///
    /// A `Serializer` may write checksums or additional framing data and fail with
    /// this error during finalization after the value has been serialized.
    Finalization(E),

    /// A deserialized leaf value was found to be invalid.
    ///
    /// The `validate` callback returned an error message.
    Invalid(usize, &'static str),
}

impl<E: core::fmt::Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Absent(depth) => {
                write!(f, "Path not currently present (depth: {depth})")
            }
            Error::TooShort(depth) => {
                write!(f, "Path too short (depth: {depth})")
            }
            Error::NotFound(depth) => {
                write!(f, "Path not found (depth: {depth})")
            }
            Error::TooLong(depth) => {
                write!(f, "Path too long (depth: {depth})")
            }
            Error::Inner(depth, error) => {
                write!(f, "(De)serialization error (depth: {depth}): {error}")
            }
            Error::Finalization(error) => {
                write!(f, "(De)serializer finalization error: {error}")
            }
            Error::Access(depth, msg) => {
                write!(f, "Access failed (depth: {depth}): {msg}")
            }
            Error::Invalid(depth, msg) => {
                write!(f, "Invalid value (depth: {depth}): {msg}")
            }
        }
    }
}

impl<T> From<T> for Error<T> {
    fn from(value: T) -> Self {
        Error::Inner(0, value)
    }
}

/// Pass an [`Error`] up one hierarchy depth level, incrementing its usize depth field by one.
pub fn increment_error<E>(err: Error<E>) -> Error<E> {
    match err {
        Error::Absent(i) => Error::Absent(i + 1),
        Error::TooShort(i) => Error::TooShort(i + 1),
        Error::NotFound(i) => Error::NotFound(i + 1),
        Error::TooLong(i) => Error::TooLong(i + 1),
        Error::Access(i, msg) => Error::Access(i + 1, msg),
        Error::Invalid(i, msg) => Error::Invalid(i + 1, msg),
        Error::Inner(i, e) => Error::Inner(i + 1, e),
        Error::Finalization(e) => Error::Finalization(e),
    }
}

/// Pass a [`Result`] up one hierarchy depth level, incrementing its usize depth field by one.
pub fn increment_result<E>(result: Result<usize, Error<E>>) -> Result<usize, Error<E>> {
    match result {
        Ok(i) => Ok(i + 1),
        Err(err) => Err(increment_error(err)),
    }
}

/// Metadata about a [TreeKey] namespace.
#[non_exhaustive]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// The maximum length of a path in bytes.
    ///
    /// This is the concatenation of all names in a path
    /// and does not include separators.
    /// It includes paths that may be [`Error::Absent`] at runtime.
    pub max_length: usize,

    /// The maximum path depth.
    ///
    /// This is equal to the maximum number of path hierarchy separators.
    /// It may be smaller than the [Tree recursion depth const generic paramerter `Y`](TreeKey#recursion).
    /// It includes paths that may be [`Error::Absent`] at runtime.
    pub max_depth: usize,

    /// The total number of paths.
    ///
    /// This includes paths that may be [`Error::Absent`] at runtime.
    pub count: usize,
}

impl Metadata {
    /// Add separator length to the maximum path length.
    ///
    /// To obtain an upper bound on the maximum length of all paths
    /// including separators, this adds `max_depth*separator_length`.
    #[inline]
    pub fn separator(self, separator: &str) -> Self {
        Self {
            max_length: self.max_length + self.max_depth * separator.len(),
            ..self
        }
    }
}

/// Traversal, iteration, and serialization/deserialization of nodes in a tree.
///
/// The following documentation sections on `TreeKey<Y>` apply analogously to `TreeSerialize<Y>`
/// and `TreeDeserialize<Y>`.
///
/// # Recursion
///
/// The `TreeKey` trait (and the `TreeSerialize`/`TreeDeserialize` traits as well)
/// are meant to be implemented
/// recursively on nested data structures. Recursion here means that a container
/// that implements `TreeKey`, may call on the `TreeKey` implementations of
/// inner types.
///
/// The const parameter `Y` in the traits here is the recursion depth and determines the
/// maximum nesting of `TreeKey` layers. It's at least `1` and defaults to `1`.
///
/// The recursion depth `Y` doubles as an upper bound to the key length
/// (the depth/height of the tree):
/// An implementor of `TreeKey<Y>` may consume at most `Y` items from the
/// `keys` iterator argument in the recursive methods ([`TreeSerialize::serialize_by_key()`],
/// [`TreeDeserialize::deserialize_by_key()`], [`TreeKey::traverse_by_key()`]). This includes
/// both the items consumed directly before recursing and those consumed indirectly
/// by recursing into inner types. In the same way it may call `func` in
/// [`TreeKey::traverse_by_key()`] at most `Y` times, again including those calls due
/// to recursion into inner `Miniconf` types.
///
/// This implies that if an implementor `T` of `TreeKey<Y>` (with `Y >= 1`) contains and recurses into
/// an inner type using that type's `TreeKey<Z>` implementation, then `1 <= Z <= Y` must
/// hold and `T` may consume at most `Y - Z` items from the `keys` iterators and call
/// `func` at most `Y - Z` times.
///
/// # Keys
///
/// The keys used to locate nodes can be iterators over `usize` indices or `&str` names or can
/// be [`Packed`] compound indices.
///
/// * `usize` is modelled after ASN.1 Object Identifiers.
/// * `&str` keys are sequences of names, like path names. When concatenated, they are separated by
/// some path hierarchy separator, e.g. `'/'`.
/// * `Packed` is a variable bit-width compact notation of hierarchical indices.
///
/// # Derive macros
///
/// Derive macros to automatically implement the correct `TreeKey<Y>` traits on a struct `S` are available through
/// [`macro@crate::TreeKey`], [`macro@crate::TreeSerialize`], and [`macro@crate::TreeDeserialize`].
/// A shorthand derive macro that derives all three trait implementations is also available at [`macro@crate::Tree`].
///
/// To derive `TreeSerialize`/`TreeDeserialize`, each field in the struct must either implement
/// [`serde::Serialize`]/[`serde::de::DeserializeOwned`]
/// (and ultimately also be supported by the intended [`serde::Serializer`]/[`serde::Deserializer`] backend)
/// or implement the respective `TreeSerialize`/`TreeDeserialize` trait themselves for the required remaining
/// recursion depth.
///
/// For each field, the remaining recursion depth is configured through the `#[tree(depth=Y)]`
/// attribute, with `Y = 0` being the implied default.
/// If `Y = 0`, the field is a leaf and accessed only through its
/// [`serde::Serialize`]/[`serde::Deserialize`] implementation.
/// With `Y > 0` the field is accessed through its `TreeKey<Y>` implementation with the given
/// remaining recursion depth.
///
/// Fields may be omitted from the derived `Tree` trait implementations using the `skip` attribute
/// (`#[tree(skip)]`).
///
/// The type to use when accessing the field through `TreeKey` can be overridden using the `typ`
/// derive macro attribute (`#[tree(typ="[f32; 4]")]`). Together with the `getter` and `setter` attributes
/// which override the `TreeSerialize` and `TreeDeserialize` accessors this can be used to customize
/// the field behavior.
///
/// # Array
///
/// Blanket implementations of the `TreeKey` traits are provided for homogeneous arrays [`[T; N]`](core::array)
/// up to recursion depth `Y = 8`.
///
/// When a [`[T; N]`](core::array) is used as `TreeKey<Y>` (i.e. marked as `#[tree(depth=Y)]` in a struct)
/// and `Y > 1` each item of the array is accessed as a `TreeKey` tree.
/// For a depth `Y = 0` (attribute absent), the entire array is accessed as one atomic
/// value. For `Y = 1` each index of the array is is instead accessed as
/// an atomic value.
///
/// The type to use depends on the desired semantics of the data contained in the array. If the array
/// contains `TreeKey` items, one can (and often wants to) use `Y >= 2`.
/// However, if each element in the array should be individually configurable as a single value (e.g. a list
/// of `u32`), then `Y = 1` can be used. With `Y = 0` all items are to be accessed simultaneously and atomically.
/// For e.g. `[[T; 2]; 3] where T: TreeKey<3>` the recursion depth is `Y = 3 + 1 + 1 = 5`.
/// It automatically implements `TreeKey<5>`.
/// For `[[T; 2]; 3] where T: Serialize + DeserializeOwned`, any `Y <= 2` is implemented.
///
/// # Option
///
/// Blanket implementations of the `TreeKey` traits are provided for [`Option<T>`]
/// up to recursion depth `Y = 8`.
///
/// These implementation do not alter the path hierarchy and do not consume any items from the `keys`
/// iterators. The `TreeKey` behavior of an [`Option`] is such that the `None` variant makes the corresponding part
/// of the tree inaccessible at run-time. It will still be iterated over by [`TreeKey::iter_paths()`] but attempts
/// to [`TreeSerialize::serialize_by_key()`] or [`TreeDeserialize::deserialize_by_key()`]
/// return [`Error::Absent`].
/// This is intended as a mechanism to provide run-time construction of the namespace. In some
/// cases, run-time detection may indicate that some component is not present. In this case,
/// the nodes will not be exposed for serialization/deserialization.
///
/// If the depth specified by the `#[tree(depth=Y)]` attribute exceeds 1,
/// the `Option` can be used to access within the inner type using its `TreeKey` trait.
/// If there is no `tree` attribute on an `Option` field in a `struct or in an array,
/// JSON `null` corresponds to `None` as usual and the `TreeKey` trait is not used.
///
/// The following example shows potential usage of arrays and `Option`:
///
/// ```
/// # use miniconf::TreeKey;
/// #[derive(TreeKey)]
/// struct S {
///     // "/b/1/2" = 5
///     #[tree(depth=2)]
///     b: [[u32; 3]; 3],
///     // "/c/0" = [3,4], optionally absent at runtime
///     #[tree(depth=2)]
///     c: [Option<[u32; 2]>; 2],
/// }
/// ```
///
/// # Generics
///
/// The macros add bounds to generic types of the struct they are acting on.
/// If a generic type parameter `T` of the struct `S<T>`is used as a type parameter to a
/// field type `a: F1<F2<T>>` the type `T` will be considered to reside at type depth `X = 2` (as it is
/// within `F2` which is within `F1`) and the following bounds will be applied:
///
/// * With the `#[tree()]` attribute not present on `a`, `T` will receive bounds `Serialize`/`DeserializeOwned` when
///   `TreeSerialize`/`TreeDeserialize` is derived.
/// * With `#[tree(depth=Y)]`, and `Y - X < 1` it will also receive bounds `Serialize + DeserializeOwned`.
/// * For `Y - X >= 1` it will receive the bound `T: TreeKey<Y - X>`.
///
/// E.g. In the following `T` resides at depth `2` and `T: TreeKey<1>` will be inferred:
///
/// ```
/// # use miniconf::TreeKey;
/// #[derive(TreeKey)]
/// struct S<T> {
///     #[tree(depth=3)]
///     a: [Option<T>; 2],
/// };
/// // This works as [u32; N] implements TreeKey<1>:
/// S::<[u32; 5]>::metadata();
/// // This does not compile as u32 does not implement TreeKey<1>:
/// // S::<u32>::metadata();
/// ```
///
/// This behavior is upheld by and compatible with all implementations in this crate. It is only violated
/// when deriving `TreeKey` for a struct that (a) forwards its own type parameters as type
/// parameters to its field types, (b) uses `TreeKey` on those fields, and (c) those field
/// types use their type parameters at other depths than `TreeKey<Y - 1>`. See also the
/// `test_derive_macro_bound_failure` test in `tests/generics.rs`.
///
/// # Validation/Getter/Setter
///
/// The [`macro@crate::TreeSerialize`]/[`macro@crate::TreeDeserialize`] derive macros supports per-field
/// getter/setter callbacks that can be used to implement validation or support remote types.
///
/// # Examples
///
/// See the [`crate`] documentation for an example showing how the traits and the derive macros work.
pub trait TreeKey<const Y: usize = 1> {
    /// The number of top-level nodes.
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// }
    /// assert_eq!(S::len(), 2);
    /// ```
    fn len() -> usize;

    /// Convert a node name to a node index.
    ///
    /// The details of the mapping and the `usize` index values
    /// are an implementation detail and only need to be stable for at runtime.
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     bar: u16,
    /// }
    /// assert_eq!(S::name_to_index("bar"), Some(1));
    /// ```
    fn name_to_index(name: &str) -> Option<usize>;

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
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// assert_eq!(
    ///     S::traverse_by_key(["bar", "0"].into_iter(), |index, name, len| {
    ///         if name == "bar" {
    ///             assert_eq!((1, 2), (index, len));
    ///         } else {
    ///             assert_eq!((0, "0", 2), (index, name, len));
    ///         }
    ///         Ok::<_, ()>(())
    ///     }),
    ///     Ok(2)
    /// );
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `func`: A `FnMut` to be called for each node on the path. Its arguments are
    ///   the index and the name of the node at the given depth. Returning `Err()` aborts
    ///   the traversal.
    ///
    /// # Returns
    /// Final node depth on success
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        // Writing this to return an iterator instead of using a callback
        // would have worse performance (O(n^2) instead of O(n) for matching)
        F: FnMut(usize, &str, usize) -> Result<(), E>;

    /// Get metadata about the paths in the namespace.
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let m = S::metadata();
    /// assert_eq!((m.max_depth, m.max_length, m.count), (2, 4, 3));
    /// ```
    fn metadata() -> Metadata;

    /// Convert keys to path.
    ///
    /// This is typically called through a [PathIter] returned by [TreeKey::iter_paths].
    ///
    /// `keys` may be longer than required. Extra items are ignored.
    ///
    /// ```
    /// # #[cfg(feature = "std")] {
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = String::new();
    /// S::path([1, 1], &mut s, "/").unwrap();
    /// assert_eq!(s, "/bar/1");
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `path`: A string to write the separators and node names into.
    ///   See also [TreeKey::metadata()] for upper bounds on path length.
    /// * `separator`: The path hierarchy separator to be inserted before each name.
    ///
    /// # Returns
    /// Final node depth on success
    fn path<K, P>(keys: K, mut path: P, separator: &str) -> Result<usize, Error<core::fmt::Error>>
    where
        K: IntoKeys,
        P: Write,
    {
        Self::traverse_by_key(keys.into_keys(), |_index, name, _len| {
            path.write_str(separator).and_then(|_| path.write_str(name))
        })
    }

    /// Convert keys to `indices`.
    ///
    /// This determines the `indices` of the item specified by `keys`.
    ///
    /// See also [`TreeKey::path()`] for the analogous function.
    ///
    /// Entries in `indices` at and beyond the `depth` returned are unaffected.
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut i = [0; 2];
    /// let depth = S::indices(["bar", "1"], &mut i).unwrap();
    /// assert_eq!(&i[..depth], [1, 1]);
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `indices`: An iterator of `&mut usize` to write the node indices into.
    ///   If `indices` is shorter than the node depth, [`Error::Inner`] is returned
    ///   `Y` is an upper bound for the required depth. See also
    ///   [TreeKey::metadata()] for an exact value.
    ///
    /// # Returns
    /// Final node depth on success
    fn indices<'a, K, I>(keys: K, indices: I) -> Result<usize, Error<()>>
    where
        K: IntoKeys,
        I: IntoIterator<Item = &'a mut usize>,
    {
        let mut indices = indices.into_iter();
        Self::traverse_by_key(keys.into_keys(), |index, _name, _len| {
            let idx = indices.next().ok_or(())?;
            *idx = index;
            Ok(())
        })
    }

    /// Convert keys to packed usize bitfield representation.
    ///
    /// See also [`Packed`].
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 5],
    /// };
    /// let (p, _) = S::packed(["bar", "4"]).unwrap();
    /// assert_eq!(p.into_lsb().get(), 0b1_1_100);
    /// let mut s = String::new();
    /// S::path(p, &mut s, "/").unwrap();
    /// assert_eq!(s, "/bar/4");
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    ///
    /// # Returns
    /// The packed indices representation on success and the leaf depth.
    fn packed<K>(keys: K) -> Result<(Packed, usize), Error<()>>
    where
        K: IntoKeys,
    {
        let mut packed = Packed::default();
        let depth = Self::traverse_by_key(keys.into_keys(), |index, _name, len| {
            packed
                .push_lsb(Packed::bits_for(len.saturating_sub(1)), index)
                .ok_or(())
                .and(Ok(()))
        })?;
        Ok((packed, depth))
    }

    /// Create an iterator of all possible paths.
    ///
    /// This is a depth-first walk.
    /// The iterator will walk all paths, including those that may be absent at
    /// run-time (see [`TreeKey#option`]).
    /// The iterator has an exact and trusted [`Iterator::size_hint()`].
    ///
    /// ```
    /// # #[cfg(feature = "std")] {
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let paths: Vec<String> = S::iter_paths("/").map(|p| p.unwrap()).collect();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    /// # }
    /// ```
    ///
    /// # Generics
    /// * `P`  - The type to hold the path. Needs to be `core::fmt::Write + Default`
    ///
    /// # Args
    /// * `separator` - The path hierarchy separator
    ///
    /// # Returns
    /// An iterator of paths with a trusted and exact [`Iterator::size_hint()`].
    fn iter_paths<P: Write>(separator: &str) -> PathIter<'_, Self, Y, P> {
        PathIter::new(separator)
    }

    /// Create an iterator of all possible indices.
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let indices: Vec<_> = S::iter_indices().collect();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    /// ```
    ///
    /// # Returns
    /// An iterator of indices with a trusted and exact [`Iterator::size_hint()`].
    fn iter_indices() -> IndexIter<Self, Y> {
        IndexIter::default()
    }

    /// Create an iterator of all packed indices.
    ///
    /// ```
    /// # use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let packed: Vec<_> = S::iter_packed().map(|p| p.unwrap().into_lsb().get()).collect();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    /// ```
    ///
    /// # Returns
    /// An iterator of packed indices.
    fn iter_packed() -> PackedIter<Self, Y> {
        PackedIter::default()
    }
}

/// TODO
///
/// ```
/// # use miniconf::{TreeAny, TreeKey};
/// #[derive(TreeKey, TreeAny, Default)]
/// struct S {
///     foo: u32,
///     #[tree(depth=1)]
///     bar: [u16; 2],
/// };
/// let s = S::default();
/// for (k, depth) in S::iter_indices() {
///     let a = s.get_by_key(k.into_iter().take(depth)).unwrap();
///     eprintln!("{:?}", (&*a).type_id());
/// }
/// ```
pub trait TreeAny<const Y: usize = 1>: TreeKey<Y> {
    /// ```
    /// # use miniconf::{TreeAny, TreeKey};
    /// #[derive(TreeKey, TreeAny, Default)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let s = S { foo: 9, bar: [11, 3] };
    /// let a = s.get_by_key(["bar", "1"].into_iter()).unwrap();
    /// assert_eq!(*a.downcast_ref::<u16>().unwrap(), 3);
    /// ```
    fn get_by_key<K>(&self, keys: K) -> Result<&dyn Any, Error<()>>
    where
        K: Keys;

    /// ```
    /// # use miniconf::{TreeAny, TreeKey};
    /// #[derive(TreeKey, TreeAny, Default)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = S::default();
    /// let a = s.get_mut_by_key(["bar", "1"].into_iter()).unwrap();
    /// let a = a.downcast_mut().unwrap();
    /// *a = 3u16;
    /// assert_eq!(s.bar[1], 3);
    /// ```
    fn get_mut_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Error<()>>
    where
        K: Keys;
}

/// Serialize a leaf node by its keys.
///
/// See also [`crate::JsonCoreSlash`] or `Postcard` for convenient
/// super traits with blanket implementations using this trait.
///
/// # Derive macro
///
/// [`macro@crate::TreeSerialize`] derives `TreeSerialize` for structs with named fields and tuple structs.
/// The `depth` and `skip` field attributes are described in the [`TreeKey`] trait.
///
/// ## `getter` field attribute
///
/// The getter is called during `serialize_by_key()` before leaf serialization.
/// Its signature is `fn(&self) -> Result<&T, &str>`.
/// The default getter is `Ok(&self.field)`.
/// Getters can be used for both
/// leaf fields as well as internal (non-leaf) fields.
/// If a getter returns an error message `Err(&str)` the serialization is not performed,
/// further getters at greater depth are invoked
/// and [`Error::InvalidLeaf`] or [`Error::InvalidInternal`] is returned
/// from `serialize_by_key()` depending on which getter failed.
pub trait TreeSerialize<const Y: usize = 1>: TreeKey<Y> {
    /// Serialize a node by keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// # use miniconf::{TreeSerialize, TreeKey};
    /// #[derive(TreeKey, TreeSerialize)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let s = S { foo: 9, bar: [11, 3] };
    /// let mut buf = [0u8; 10];
    /// let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    /// s.serialize_by_key(["bar", "0"].into_iter(), &mut ser).unwrap();
    /// let length = ser.end();
    /// assert_eq!(&buf[..length], b"11");
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `ser`: A `Serializer` to to serialize the value.
    ///
    /// # Returns
    /// Node depth on success.
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer;
}

/// Deserialize a leaf node by its keys.
///
/// See also [`crate::JsonCoreSlash`] for a convenient blanket implementation using this trait.
///
/// # Derive macro
///
/// [`macro@crate::TreeDeserialize`] derives `TreeSerialize` for structs with named fields and tuple structs.
/// The `depth` and `skip` field attributes are described in the [`TreeKey`] trait.
///
/// ## `setter` field attribute
///
/// For internal (non-leaf) fields the setter is invoked before deserialization
/// while traversing down to the leaf node.
/// The internal setter signature is `fn(&mut self) -> Result<&mut T, &str>`.
/// The default internal setter is `Ok(&mut self.field)`.
/// Note that when a non-leaf setter is invoked, deserialization and leaf
/// value update have not yet taken place.
/// If an internal setter returns an `Err`
/// no further setters are invoked and [`Error::InvalidInternal`] will be returned
/// from `deserialize_by_key()`.
///
/// For leaf fields the setter is invoked during `deserialize_by_key()` after
/// successful deserialization but before updating the leaf value.
/// The setter signature is `fn(&mut self, new: T) -> Result<(), &str>`
/// The default leaf setter is `{ self.field = new; Ok(()) }`.
/// If the leaf setter returns an `Err(&str)`
/// [`Error::InvalidLeaf`] is returned from `deserialize_by_key()`.
///
/// Note: In both cases the setters receive `&mut self` as an argument and may
/// mutate the struct.
///
/// ```
/// # use miniconf::{Error, Tree, JsonCoreSlash};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(validate=leaf)]
///     a: f32,
///     #[tree(depth=1, validate=non_leaf)]
///     b: [f32; 2],
/// };
/// fn leaf(s: &mut S, new: f32) -> Result<f32, &'static str> {
///     Err("fail")
/// }
/// fn non_leaf(s: &mut S, depth: usize) -> Result<usize, &'static str> {
///     Err("fail")
/// }
/// ```
pub trait TreeDeserialize<'de, const Y: usize = 1>: TreeKey<Y> {
    /// Deserialize an node by keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// # use miniconf::{TreeDeserialize, TreeKey};
    /// #[derive(Default, TreeKey, TreeDeserialize)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = S::default();
    /// let mut de = serde_json_core::de::Deserializer::new(b"7");
    /// s.deserialize_by_key(["bar", "0"].into_iter(), &mut de).unwrap();
    /// de.end().unwrap();
    /// assert_eq!(s.bar[0], 7);
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `de`: A `Deserializer` to deserialize the value.
    ///
    /// # Returns
    /// Node depth on success
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>;
}
