use core::any::Any;
use core::fmt::Write;

use crate::{Error, IndexIter, IntoKeys, Keys, Packed, PackedIter, PathIter, Traversal};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Metadata about a [TreeKey] namespace.
///
/// Metadata includes paths that may be [`Traversal::Absent`] at runtime.
#[non_exhaustive]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    /// The maximum length of a path in bytes.
    ///
    /// This is the exact maximum of the length of the concatenation of all field names/indices
    /// in a path. By default, it does not include separators.
    pub max_length: usize,

    /// The maximum key depth.
    ///
    /// This is equal to the exact maximum number of path hierarchy separators.
    /// It's the exact maximum number of key indices.
    /// It may be smaller than the [`TreeKey<Y>` recursion depth](TreeKey#recursion-depth).
    pub max_depth: usize,

    /// The exact total number of keys.
    pub count: usize,
}

impl Metadata {
    /// Add separator length to the maximum path length.
    ///
    /// To obtain an upper bound on the maximum length of all paths
    /// including separators, this adds `max_depth*separator_length`.
    #[inline]
    pub fn max_length(self, separator: &str) -> usize {
        self.max_length + self.max_depth * separator.len()
    }
}

/// Traversal, iteration of keys in a tree.
///
/// See also the sub-traits [`TreeSerialize<Y>`], [`TreeDeserialize<Y>`], [`TreeAny<Y>`].
///
/// # Recursion depth
///
/// The `Tree*` traits are meant to be implemented
/// recursively on nested data structures. Recursion here means that a container
/// that implements `Tree*`, may call on the `Tree*` implementations of
/// inner types or `Serialize`/`Deserialize`/`Any` of leaf types.
///
/// The const parameter `Y` in the traits is the recursion depth and determines the
/// maximum nesting of `Tree*` layers. It's at least `1` and defaults to `1`.
///
/// The recursion depth `Y` doubles as an upper bound to the maximum key length
/// (the depth/height of the tree):
/// An implementor of `TreeKey<Y>` may consume at most `Y` items from the
/// `keys` argument. This includes
/// both the items consumed directly before recursing/terminating and those consumed
/// indirectly by recursing into inner types. In the same way it may call `func` in
/// [`TreeKey::traverse_by_key()`] at most `Y` times, again including those calls due
/// to recursion into inner `TreeKey` types.
///
/// This implies that if an implementor `T` of `TreeKey<Y>` (with `Y >= 1`) contains and recurses into
/// an inner type using that type's `TreeKey<Z>` implementation, then `1 <= Z <= Y` must
/// hold and `T` may consume at most `Y - Z` items from the `keys` iterators and call
/// `func` at most `Y - Z` times. It is recommended (but not necessary) to keep `Z = Y - 1`
/// (even if no keys are consumed directly) to satisfy the bound
/// heuristics in the derive macro.
///
/// The exact maximum key depth can be obtained through [`TreeKey::metadata()`].
///
/// # Keys
///
/// The keys used to identify nodes can be iterators over `usize` indices or `&str` names or can
/// be [`Packed`] compound indices.
///
/// * `usize` is modelled after ASN.1 Object Identifiers.
/// * `&str` keys are sequences of names, like path names. When concatenated, they are separated by
///    some path hierarchy separator, e.g. `'/'`.
/// * [`Packed`] is a variable bit-width compact compressed notation of hierarchical indices.
///
/// There is a one-to-one relationship between nodes and keys.
///
/// # Derive macros
///
/// Derive macros to automatically implement the correct traits on a struct are available through
/// [`macro@crate::TreeKey`], [`macro@crate::TreeSerialize`], [`macro@crate::TreeDeserialize`],
/// and [`macro@crate::TreeAny`].
/// A shorthand derive macro that derives all four trait implementations is also available at [`macro@crate::Tree`].
///
/// The derive macros support per-field attribute to control the derived trait implementations.
///
/// ## Depth
///
/// For each field, the recursion depth is configured through the `#[tree(depth=Y)]`
/// attribute, with `Y = 0` being the implied default.
/// If `Y = 0`, the field is a leaf and accessed only through its
/// [`Serialize`]/[`Deserialize`]/[`Any`] implementation.
/// With `Y > 0` the field is accessed through its `TreeKey<Y>` implementation with the given
/// remaining recursion depth.
///
/// ## Rename
///
/// The key for named struct fields may be changed from the default field ident using the `rename`
/// derive macro attribute (`#[tree(rename="otherName")]`).
///
/// ## Skip
///
/// Named fields may be omitted from the derived `Tree` trait implementations using the `skip` attribute
/// (`#[tree(skip)]`).
/// Note that for tuple structs skipping is only supported for terminal fields:
///
/// ```compile_fail
/// use miniconf::Tree;
/// #[derive(Tree)]
/// struct S(#[tree(skip)] (), i32);
/// ```
///
/// ```
/// use miniconf::Tree;
/// #[derive(Tree)]
/// struct S(i32, #[tree(skip)] ());
/// ```
///
/// ## Type
///
/// The type to use when accessing the field through `TreeKey` can be overridden using the `typ`
/// derive macro attribute (`#[tree(typ="[f32; 4]")]`).
///
/// ## Accessors
///
/// The `get`, `get_mut`, `validate` callbacks can be used to implement accessors,
/// validation or support remote types (e.g. `#[tree(get_mut=func)]`)
///
/// ## `get`
///
/// The getter is called during `serialize_by_key()` before leaf serialization and
/// during `ref_any_by_key()`. Its signature is `fn(&self) -> Result<&T, &'static str>`.
/// The default getter is `Ok(&self.field)`.
/// Getters can be used for both leaf fields as well as internal (non-leaf) fields.
/// If a getter returns an error message `Err(&str)` the serialization/traversal
/// is not performed, further getters at greater depth are not invoked
/// and [`Traversal::Access`] is returned.
///
/// ## `get_mut`
///
/// For internal (non-leaf) fields `get_mut` is invoked during `mut_any_by_key()` and
/// during `deserialize_by_key()` before deserialization while traversing down to
/// the leaf node.
/// For leaf fields it is invoked after deserialization and validation but before
/// updating the leaf value.
/// The signature is `fn(&mut self) -> Result<&mut T, &str>`.
/// The default `get_mut` is `Ok(&mut self.field)`.
/// If `get_mut` returns an `Err` [`Traversal::Access`] will be returned.
/// If a leaf `get_mut` returns an `Err` the leaf node is not updated in
/// `deserialize_by_key()`.
///
/// Note: In both cases `get_mut` receives `&mut self` as an argument and may
/// mutate the struct.
///
/// ```
/// use miniconf::{Error, Tree, JsonCoreSlash};
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
///
/// ### `validate`
///
/// For leaf fields the `validate` callback is called during `deserialize_by_key()`
/// after successful deserialization of the leaf value but before `get_mut()` and
/// before storing the value.
/// The leaf `validate` signature is `fn(&mut self, value: T) ->
/// Result<T, &'static str>`. It may mutate the value before it is being stored.
/// If a leaf validate callback returns `Err(&str)`, the leaf value is not updated
/// and [`Traversal::Invalid`] is returned from `deserialize_by_key()`.
/// For internal fields `validate` is called after the successful update of the leaf field
/// during upward traversal.
/// The internal `validate` signature is `fn(&mut self, depth: usize) ->
/// Result<usize, &'static str>`
/// If an internal validate callback returns `Err()`, the leaf value **has been**
/// updated and [`Traversal::Invalid`] is returned from `deserialize_by_key()`.
///
/// Note: In both cases `validate` receives `&mut self` as an argument and may
/// mutate the struct.
///
/// ## Bounds
///
/// To derive `TreeSerialize`/`TreeDeserialize`/`TreeAny`, each field (that is not `skip`ped)
/// in the struct must either implement [`Serialize`]/[`Deserialize`]/[`Any`]
/// or implement the respective `TreeSerialize`/`TreeDeserialize`/`TreeAny` trait
/// for the required remaining recursion depth.
///
/// ## Generics
///
/// The macros add bounds to generic types of the struct they are acting on.
/// If a generic type parameter `T` of the struct `S<T>`is used as a type parameter to a
/// field type `a: F1<F2<T>>` the type `T` will be considered to reside at type depth `X = 2` (as it is
/// within `F2` which is within `F1`) and the following bounds will be applied:
///
/// * With the `#[tree()]` attribute not present on `a`, `T` will receive bounds `Serialize`/`Deserialize` when
///   `TreeSerialize`/`TreeDeserialize` is derived.
/// * With `#[tree(depth=Y)]`, and `Y - X < 1` it will receive the bounds `Serialize`/`Deserialize`.
/// * For `Y - X >= 1` it will receive the bound `T: TreeKey<Y - X>`.
///
/// E.g. In the following `T` resides at depth `2` and `T: TreeKey<1>` will be inferred:
///
/// ```
/// use miniconf::TreeKey;
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
/// # Array
///
/// Blanket implementations of the `TreeKey` traits are provided for homogeneous arrays [`[T; N]`](core::array)
/// up to recursion depth `Y = 16`.
///
/// When a [`[T; N]`](core::array) is used as `TreeKey<Y>` (i.e. marked as `#[tree(depth=Y)]` in a struct)
/// and `Y > 1` each item of the array is accessed as a `TreeKey` tree.
/// For `Y = 1` each index of the array is is instead accessed as
/// an atomic value.
/// For a depth `Y = 0` (attribute absent), the entire array is accessed as one atomic
/// value.
///
/// The `depth` to use on the array depends on the desired semantics of the data contained
/// in the array. If the array contains `TreeKey` items, you likely want use `Y >= 2`.
/// However, if each element in the array should be individually configurable as a single value (e.g. a list
/// of `u32`), then `Y = 1` can be used. With `Y = 0` all items are to be accessed simultaneously and atomically.
/// For e.g. `[[T; 2]; 3] where T: TreeKey<3>` the recursion depth is `Y = 3 + 1 + 1 = 5`.
/// It automatically implements `TreeKey<5>`.
/// For `[[T; 2]; 3]` with `T: Serialize`/`T: Deserialize`/`T: Any` any `Y <= 2` trait is implemented.
///
/// # Option
///
/// Blanket implementations of the `TreeKey` traits are provided for [`Option<T>`]
/// up to recursion depth `Y = 16`.
///
/// These implementations do not alter the path hierarchy and do not consume any items from the `keys`
/// iterators. The `TreeKey` behavior of an [`Option`] is such that the `None` variant makes the corresponding part
/// of the tree inaccessible at run-time. It will still be iterated over (e.g. by [`TreeKey::iter_paths()`]) but attempts
/// to access it (e.g. [`TreeSerialize::serialize_by_key()`], [`TreeDeserialize::deserialize_by_key()`],
/// [`TreeAny::ref_any_by_key()`], or [`TreeAny::mut_any_by_key()`])
/// return the special [`Traversal::Absent`].
/// This is intended as a mechanism to provide run-time construction of the namespace. In some
/// cases, run-time detection may indicate that some component is not present. In this case,
/// the nodes will not be exposed for serialization/deserialization.
///
/// If the depth specified by the `#[tree(depth=Y)]` attribute exceeds 1,
/// the `Option` can be used to access within the inner type using its `TreeKey` trait.
/// If there is no `tree` attribute on an `Option` field in a `struct or in an array,
/// JSON `null` corresponds to `None` as usual and the `TreeKey` trait is not used.
///
/// # Examples
///
/// See the [`crate`] documentation for a longer example showing how the traits and the derive macros work.
pub trait TreeKey<const Y: usize = 1> {
    /// Compute metadata about all paths.
    ///
    /// ```
    /// use miniconf::TreeKey;
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

    /// Traverse from the root to a leaf and call a function for each node.
    ///
    /// Traversal is aborted once `func` returns an `Err(E)`.
    ///
    /// This may not exhaust `keys` if a leaf is found early. i.e. `keys`
    /// may be longer than required: `Traversal(TooLong)` is never returned.
    /// This is to optimize path iteration (downward probe).
    /// If `Self` is a leaf, nothing will be consumed from `keys`
    /// and `Ok(0)` will be returned.
    /// If `keys` is exhausted before reaching a leaf node,
    /// `Err(Traversal(TooShort(depth)))` is returned.
    ///
    /// ```
    /// use miniconf::{TreeKey, IntoKeys};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut ret = [(1, Some("bar"), 2), (0, None, 2)].into_iter();
    /// let func = |index, name, len| -> Result<(), ()> {
    ///         assert_eq!(ret.next().unwrap(), (index, name, len));
    ///         Ok(())
    /// };
    /// assert_eq!(S::traverse_by_key(["bar", "0"].into_keys(), func), Ok(2));
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `func`: A `FnMut` to be called for each (internal and leaf) node on the path.
    ///   Its arguments are the index and the optional name of the node and the number
    ///   of top-level nodes at the given depth. Returning `Err()` aborts the traversal.
    ///
    /// # Returns
    /// Final node depth on success (the number of keys consumed, number of calls to `func`)
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        // Writing this to return an iterator instead of using a callback
        // would have worse performance (O(n^2) instead of O(n) for matching)
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>;

    /// Convert keys to path.
    ///
    /// The keys can be
    /// * too short: the internal node is returned
    /// * matched length: the leaf node is returned
    /// * too long: the leaf node is returned
    ///
    /// ```
    /// use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = String::new();
    /// S::path([1, 1], &mut s, "/").unwrap();
    /// assert_eq!(s, "/bar/1");
    /// ```
    ///
    /// # Args
    /// * `keys`: `IntoKeys` to identify the node.
    /// * `path`: A `Write` to write the separators and node names into.
    ///   See also [TreeKey::metadata()] and [`Metadata::max_length()`] for upper bounds
    ///   on path length.
    /// * `separator`: The path hierarchy separator to be inserted before each name.
    ///
    /// # Returns
    /// Node depth on success
    fn path<K, P>(keys: K, mut path: P, separator: &str) -> Result<usize, Error<core::fmt::Error>>
    where
        K: IntoKeys,
        P: Write,
    {
        let func = |index, name: Option<_>, _len| {
            path.write_str(separator)?;
            path.write_str(name.unwrap_or(itoa::Buffer::new().format(index)))
        };
        match Self::traverse_by_key(keys.into_keys(), func) {
            Ok(depth)
            | Err(Error::Traversal(Traversal::TooShort(depth) | Traversal::TooLong(depth))) => {
                Ok(depth)
            }
            Err(err) => Err(err),
        }
    }

    /// Return the keys formatted as a normalized JSON path.
    ///
    /// * Named fields (struct) are encoded in dot notation.
    /// * Indices (tuple struct, array) are encoded in index notation
    ///
    /// See also [`TreeKey::path()`].
    ///
    /// ```
    /// use miniconf::{TreeKey, JsonPath};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = String::new();
    /// let idx = [1, 1];
    /// S::json_path(idx, &mut s).unwrap();
    /// assert_eq!(s, ".bar[1]");
    ///
    /// let (indices, depth) = S::indices(JsonPath::from(&s)).unwrap();
    /// assert_eq!(&indices[..depth], idx);
    /// ```
    fn json_path<K, P>(keys: K, mut path: P) -> Result<usize, Error<core::fmt::Error>>
    where
        K: IntoKeys,
        P: Write,
    {
        let func = |index, name, _len| match name {
            Some(name) => {
                path.write_char('.')?;
                path.write_str(name)
            }
            None => {
                path.write_char('[')?;
                path.write_str(itoa::Buffer::new().format(index))?;
                path.write_char(']')
            }
        };
        match Self::traverse_by_key(keys.into_keys(), func) {
            Ok(depth)
            | Err(Error::Traversal(Traversal::TooShort(depth) | Traversal::TooLong(depth))) => {
                Ok(depth)
            }
            Err(err) => Err(err),
        }
    }

    /// Convert keys to `indices`.
    ///
    /// See also [`TreeKey::path()`].
    ///
    /// ```
    /// use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let (indices, depth) = S::indices(["bar", "1"]).unwrap();
    /// assert_eq!(&indices[..depth], [1, 1]);
    /// ```
    ///
    /// # Returns
    /// Indices and depth on success
    fn indices<K>(keys: K) -> Result<([usize; Y], usize), Traversal>
    where
        K: IntoKeys,
    {
        let mut indices = [0; Y];
        let mut it = indices.iter_mut();
        let func = |index, _name, _len| -> Result<(), ()> {
            let idx = it.next().ok_or(())?;
            *idx = index;
            Ok(())
        };
        match Self::traverse_by_key(keys.into_keys(), func) {
            Ok(depth)
            | Err(Error::Traversal(Traversal::TooShort(depth) | Traversal::TooLong(depth))) => {
                Ok((indices, depth))
            }
            Err(err) => Err(Traversal::try_from(err).unwrap()),
        }
    }

    /// Convert keys to packed usize bitfield representation.
    ///
    /// See also [`Packed`] and [`TreeKey::path()`].
    ///
    /// ```
    /// use miniconf::TreeKey;
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
    /// # Returns
    /// The packed indices representation and depth on success.
    fn packed<K>(keys: K) -> Result<(Packed, usize), Error<()>>
    where
        K: IntoKeys,
    {
        let mut packed = Packed::default();
        let func = |index, _name, len: usize| match packed
            .push_lsb(Packed::bits_for(len.saturating_sub(1)), index)
        {
            None => Err(()),
            Some(_) => Ok(()),
        };
        match Self::traverse_by_key(keys.into_keys(), func) {
            Ok(depth)
            | Err(Error::Traversal(Traversal::TooShort(depth) | Traversal::TooLong(depth))) => {
                Ok((packed, depth))
            }
            Err(err) => Err(err),
        }
    }

    /// Create an iterator of all possible leaf paths.
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [`TreeKey#option`]).
    /// An iterator with an exact and trusted `size_hint()` can be obtained from
    /// this through [`PathIter::count()`].
    ///
    /// ```
    /// use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let paths: Vec<String> = S::iter_paths("/").count().map(|p| p.unwrap()).collect();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    /// ```
    ///
    /// # Generics
    /// * `P`  - The type to hold the path.
    ///
    /// # Args
    /// * `separator` - The path hierarchy separator
    fn iter_paths<P: core::fmt::Write + Default>(separator: &str) -> PathIter<'_, Self, Y, P, Y> {
        PathIter::new(separator)
    }

    /// Create an iterator of all possible leaf indices.
    ///
    /// See also [`TreeKey::iter_paths()`].
    ///
    /// ```
    /// use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let indices: Vec<_> = S::iter_indices().count().collect();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    /// ```
    fn iter_indices() -> IndexIter<Self, Y, Y> {
        IndexIter::default()
    }

    /// Create an iterator of all packed leaf indices.
    ///
    /// See also [`TreeKey::iter_paths()`].
    ///
    /// ```
    /// use miniconf::TreeKey;
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let packed: Vec<_> = S::iter_packed()
    ///     .count()
    ///     .map(|p| p.unwrap().into_lsb().get())
    ///     .collect();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    /// ```
    fn iter_packed() -> PackedIter<Self, Y, Y> {
        PackedIter::default()
    }
}

/// Access any node by keys.
///
/// This uses the `dyn Any` trait object.
///
/// ```
/// use core::any::Any;
/// use miniconf::{TreeAny, TreeKey, JsonPath, IntoKeys};
/// #[derive(TreeKey, TreeAny, Default)]
/// struct S {
///     foo: u32,
///     #[tree(depth=1)]
///     bar: [u16; 2],
/// };
/// let mut s = S::default();
///
/// for (key, depth) in S::iter_indices() {
///     let a = s.ref_any_by_key(key[..depth].iter().copied().into_keys()).unwrap();
///     assert!([0u32.type_id(), 0u16.type_id()].contains(&(&*a).type_id()));
/// }
///
/// let val: &mut u16 = s.mut_by_key(JsonPath::from(".bar[1]")).unwrap();
/// *val = 3;
/// assert_eq!(s.bar[1], 3);
///
/// let val: &u16 = s.ref_by_key(JsonPath::from(".bar[1]")).unwrap();
/// assert_eq!(*val, 3);
/// ```
pub trait TreeAny<const Y: usize = 1>: TreeKey<Y> {
    /// Obtain a reference to a `dyn Any` trait object for a leaf node.
    fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys;

    /// Obtain a mutable reference to a `dyn Any` trait object for a leaf node.
    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys;

    /// Obtain a reference to a leaf of known type by key.
    fn ref_by_key<T: Any, K: IntoKeys>(&self, keys: K) -> Result<&T, Traversal> {
        self.ref_any_by_key(keys.into_keys())?
            .downcast_ref()
            .ok_or(Traversal::Invalid(0, "Incorrect type"))
    }

    /// Obtain a mutable reference to a leaf of known type by key.
    fn mut_by_key<T: Any, K: IntoKeys>(&mut self, keys: K) -> Result<&mut T, Traversal> {
        self.mut_any_by_key(keys.into_keys())?
            .downcast_mut()
            .ok_or(Traversal::Invalid(0, "Incorrect type"))
    }
}

// # Alternative serialize/deserialize designs
//
// One could have (ab)used a custom `Serializer`/`Deserializer` wrapper for this but that would be inefficient:
// `Serialize` would try to pass each node to the `Serializer` until the `Serializer` matches the leaf key
// (and could terminate early).

/// Serialize a leaf node by its keys.
///
/// See also [`crate::JsonCoreSlash`] or `Postcard` for convenient
/// super traits with blanket implementations using this trait.
///
/// # Derive macro
///
/// [`macro@crate::TreeSerialize`] derives `TreeSerialize` for structs with named fields and tuple structs.
/// The field attributes are described in the [`TreeKey`] trait.
pub trait TreeSerialize<const Y: usize = 1>: TreeKey<Y> {
    /// Serialize a node by keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// use miniconf::{TreeSerialize, TreeKey, IntoKeys};
    /// #[derive(TreeKey, TreeSerialize)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let s = S { foo: 9, bar: [11, 3] };
    /// let mut buf = [0u8; 10];
    /// let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    /// s.serialize_by_key(["bar", "0"].into_keys(), &mut ser).unwrap();
    /// let len = ser.end();
    /// assert_eq!(&buf[..len], b"11");
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
/// The field attributes are described in the [`TreeKey`] trait.
pub trait TreeDeserialize<'de, const Y: usize = 1>: TreeKey<Y> {
    /// Deserialize a leaf node by its keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// use miniconf::{TreeDeserialize, TreeKey, IntoKeys};
    /// #[derive(Default, TreeKey, TreeDeserialize)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = S::default();
    /// let mut de = serde_json_core::de::Deserializer::new(b"7");
    /// s.deserialize_by_key(["bar", "0"].into_keys(), &mut de).unwrap();
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
