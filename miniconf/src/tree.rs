use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{Error, IntoKeys, Keys, Node, NodeIter, Transcode, Traversal, Walk};

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
/// This implies that if an implementor `T` of `TreeKey<Y>` (with `Y >= 1`) contains and
/// recurses into an inner type using that type's `TreeKey<Z>` implementation, then
/// `1 <= Z <= Y` must hold and `T` may consume at most `Y - Z` items from the `keys`
/// iterators and call `func` at most `Y - Z` times. It is recommended (but not necessary)
/// to keep `Z = Y - 1` (even if no keys are consumed directly) to satisfy the bound
/// heuristics in the derive macro.
///
/// The exact maximum key depth can be obtained through [`TreeKey::traverse_all()`].
///
/// # Keys
///
/// There is a one-to-one relationship between nodes and keys.
/// The keys used to identify nodes support [`Keys`]/[`IntoKeys`]. They can be
/// obtained from other [`IntoKeys`] through [`Transcode`]/[`TreeKey::transcode()`].
/// An iterator of keys for the nodes is available through [`TreeKey::nodes()`]/[`NodeIter`].
///
/// * `usize` is modelled after ASN.1 Object Identifiers, see [`crate::Indices`].
/// * `&str` keys are sequences of names, like path names. When concatenated, they are separated
///   by some path hierarchy separator, e.g. `'/'`, see [`crate::Path`], or by some more
///   complex notation, see [`crate::JsonPath`].
/// * [`crate::Packed`] is a variable bit-width compact compressed notation of
///   hierarchical compound indices.
///
/// # Derive macros
///
/// Derive macros to automatically implement the correct traits on a struct are available through
/// [`macro@crate::TreeKey`], [`macro@crate::TreeSerialize`], [`macro@crate::TreeDeserialize`],
/// and [`macro@crate::TreeAny`].
/// A shorthand derive macro that derives all four trait implementations is also available at
/// [`macro@crate::Tree`].
///
/// The derive macros support per-field attribute to control the derived trait implementations.
///
/// ## Depth
///
/// For each field, the recursion depth is configured through the `depth`
/// attribute, with `Y = 0` being the implied default.
/// If `Y = 0`, the field is a leaf and accessed only through its
/// [`serde::Serialize`]/[`serde::Deserialize`]/[`Any`] implementation.
/// With `Y > 0` the field is accessed through its `TreeKey<Y>` implementation with the given
/// recursion depth.
///
/// ## Rename
///
/// The key for named struct fields may be changed from the default field ident using the `rename`
/// derive macro attribute.
///
/// ```
/// use miniconf::{Path, Tree, TreeKey};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(rename = "OTHER")]
///     a: f32,
/// };
/// let (name, _node) = S::transcode::<Path<String, '/'>, _>([0usize]).unwrap();
/// assert_eq!(name.as_str(), "/OTHER");
/// ```
///
/// ## Skip
///
/// Named fields may be omitted from the derived `Tree` trait implementations using the
/// `skip` attribute.
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
/// If an internal node validate callback returns `Err()`, the leaf value **has been**
/// updated and [`Traversal::Invalid`] is returned from `deserialize_by_key()`.
///
/// Note: In both cases `validate` receives `&mut self` as an argument and may
/// mutate the struct.
///
/// ```
/// use miniconf::{Error, Tree};
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
/// ## Bounds
///
/// To derive `TreeSerialize`/`TreeDeserialize`/`TreeAny`, each field (that is not `skip`ped)
/// in the struct must either implement [`serde::Serialize`]/[`serde::Deserialize`]/[`Any`]
/// or implement the respective `TreeSerialize`/`TreeDeserialize`/`TreeAny` trait
/// for the required remaining recursion depth.
///
/// ## Generics
///
/// The macros add bounds to generic types of the struct they are acting on.
/// If a generic type parameter `T` of the struct `S<T>`is used as a type parameter to a
/// field type `a: F1<F2<T>>` the type `T` will be considered to reside at type depth `X = 2`
/// (as it is within `F2` which is within `F1`) and the following bounds will be applied:
///
/// * With the `#[tree()]` attribute not present on `a`, `T` will receive bounds
///   `Serialize`/`Deserialize` when `TreeSerialize`/`TreeDeserialize` is derived.
/// * With `#[tree(depth=Y)]`, and `Y - X < 1` it will receive the bounds `Serialize`/`Deserialize`.
/// * For `Y - X >= 1` it will receive the bound `T: TreeKey<Y - X>`.
///
/// E.g. In the following `T` resides at depth `2` and `T: TreeKey<1>` will be inferred:
///
/// ```
/// use miniconf::{Metadata, TreeKey};
/// #[derive(TreeKey)]
/// struct S<T> {
///     #[tree(depth = 3)]
///     a: [Option<T>; 2],
/// };
/// // This works as [u32; N] implements TreeKey<1>:
/// S::<[u32; 5]>::traverse_all::<Metadata>();
/// // This does not compile as u32 does not implement TreeKey<1>:
/// // S::<u32>::traverse_all::<Metadata>();
/// ```
///
/// This behavior is upheld by and compatible with all implementations in this crate. It is
/// only violated when deriving `TreeKey` for a struct that (a) forwards its own type parameters
/// as type parameters to its field types, (b) uses `TreeKey` on those fields, and (c) those field
/// types use their type parameters at other depths than `TreeKey<Y - 1>`. See also the
/// `test_derive_macro_bound_failure` test in `tests/generics.rs`.
///
/// # Array
///
/// Blanket implementations of the `TreeKey` traits are provided for homogeneous arrays
/// [`[T; N]`](core::array) up to recursion depth `Y = 16`.
///
/// When a `[T; N]` is used through `TreeKey<Y>` (i.e. marked as `#[tree(depth=Y)]` in a struct)
/// and `Y > 1` each item of the array is accessed as a `TreeKey` tree.
/// For `Y = 1` each index of the array is instead accessed as an atomic value.
/// For a depth `Y = 0` (attribute absent), the entire array is accessed as one atomic value.
///
/// The `depth` to use on the array depends on the desired semantics of the data contained
/// in the array. If the array contains `TreeKey` items, you likely want use `Y >= 2`.
/// However, if each element in the array should be individually configurable as a single
/// value (e.g. a list of `u32`), then `Y = 1` can be used.
/// With `Y = 0` all items are to be accessed simultaneously and atomically.
/// For e.g. `[[T; 2]; 3] where T: TreeKey<3>` the recursion depth is `Y = 3 + 1 + 1 = 5`.
/// It automatically implements `TreeKey<5>`.
/// For `[[T; 2]; 3]` with `T: Serialize`/`T: Deserialize`/`T: Any` any `Y <= 2` trait is
/// implemented.
///
/// # Option
///
/// Blanket implementations of the `TreeKey` traits are provided for [`Option<T>`]
/// up to recursion depth `Y = 16`.
///
/// These implementations do not alter the path hierarchy and do not consume any items from the `keys`
/// iterators. The `TreeKey` behavior of an [`Option`] is such that the `None` variant makes the
/// corresponding part of the tree inaccessible at run-time. It will still be iterated over (e.g.
/// by [`TreeKey::nodes()`]) but attempts to access it (e.g. [`TreeSerialize::serialize_by_key()`],
/// [`TreeDeserialize::deserialize_by_key()`], [`TreeAny::ref_any_by_key()`], or
/// [`TreeAny::mut_any_by_key()`]) return the special [`Traversal::Absent`].
///
/// If the depth specified by the `#[tree(depth=Y)]` attribute exceeds 1,
/// the `Option` can be used to access the inner type using its `TreeKey<{Y - 1}>` trait.
/// If there is no `tree` attribute on an `Option` field in a `struct or in an array,
/// JSON `null` corresponds to `None` as usual and the `TreeKey` trait is not used.
///
/// # Examples
///
/// See the [`crate`] documentation for a longer example showing how the traits and the derive
/// macros work.
pub trait TreeKey<const Y: usize = 1> {
    /// Walk metadata about all paths.
    ///
    /// ```
    /// use miniconf::{Metadata, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth = 1)]
    ///     bar: [u16; 2],
    /// };
    /// let m = S::traverse_all::<Metadata>().unwrap();
    /// assert_eq!((m.max_depth, m.max_length, m.count), (2, 4, 3));
    /// ```
    fn traverse_all<W: Walk>() -> Result<W, W::Error>;

    /// Traverse from the root to a leaf and call a function for each node.
    ///
    /// If a leaf is found early (`keys` being longer than required)
    /// `Err(Traversal(TooLong(depth)))` is returned.
    /// If `keys` is exhausted before reaching a leaf node,
    /// `Err(Traversal(TooShort(depth)))` is returned.
    ///
    /// ```
    /// use miniconf::{IntoKeys, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth = 1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut ret = [(1, Some("bar"), 2), (0, None, 2)].into_iter();
    /// let func = |index, name, len| -> Result<(), ()> {
    ///     assert_eq!(ret.next().unwrap(), (index, name, len));
    ///     Ok(())
    /// };
    /// assert_eq!(S::traverse_by_key(["bar", "0"].into_keys(), func), Ok(2));
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `func`: A `FnMut` to be called for each (internal and leaf) node on the path.
    ///   Its arguments are the index and the optional name of the node and the number
    ///   of top-level nodes at the given depth. Returning `Err(E)` aborts the traversal.
    ///   Returning `Ok(())` continues the downward traversal.
    ///
    /// # Returns
    /// Node depth on success (number of keys consumed/number of calls to `func`)
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        // Writing this to return an iterator instead of using a callback
        // would have worse performance (O(n^2) instead of O(n) for matching)
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>;

    /// Transcode keys to a new keys type representation
    ///
    /// The keys can be
    /// * too short: the internal node is returned
    /// * matched length: the leaf node is returned
    /// * too long: Err(TooLong(depth)) is returned
    ///
    /// In order to not require `N: Default`, use [`Transcode::transcode`] on
    /// an existing `&mut N`.
    ///
    /// Potential [`Transcode`] targets:
    ///
    /// * [`crate::Path`]: `char`-separated `Write`
    /// * [`crate::JsonPath`]: normalized JSON path
    /// * [`crate::Indices`]: `usize` indices array
    /// * [`crate::Packed`]: Packed `usize`` bitfield representation
    /// * `()` (the unit): Obtain just the [`Node`] information.
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth = 1)]
    ///     bar: [u16; 5],
    /// };
    ///
    /// let idx = [1usize, 1];
    ///
    /// let (path, node) = S::transcode::<Path<String, '/'>, _>(idx).unwrap();
    /// assert_eq!(path.as_str(), "/bar/1");
    /// let (path, node) = S::transcode::<JsonPath<String>, _>(idx).unwrap();
    /// assert_eq!(path.as_str(), ".bar[1]");
    /// let (indices, node) = S::transcode::<Indices<[_; 2]>, _>(&path).unwrap();
    /// assert_eq!(&indices[..node.depth()], idx);
    /// let (indices, node) = S::transcode::<Indices<[_; 2]>, _>(["bar", "1"]).unwrap();
    /// assert_eq!(&indices[..node.depth()], [1, 1]);
    /// let (packed, node) = S::transcode::<Packed, _>(["bar", "4"]).unwrap();
    /// assert_eq!(packed.into_lsb().get(), 0b1_1_100);
    /// let (path, node) = S::transcode::<Path<String, '/'>, _>(packed).unwrap();
    /// assert_eq!(path.as_str(), "/bar/4");
    /// let ((), node) = S::transcode(&path).unwrap();
    /// assert_eq!(node, Node::leaf(2));
    /// ```
    ///
    /// # Args
    /// * `keys`: `IntoKeys` to identify the node.
    ///
    /// # Returns
    /// Transcoded target and node information on success
    fn transcode<N, K>(keys: K) -> Result<(N, Node), Traversal>
    where
        K: IntoKeys,
        N: Transcode + Default,
    {
        let mut target = N::default();
        let node = target.transcode::<Self, Y, _>(keys)?;
        Ok((target, node))
    }

    /// Return an iterator over nodes of a given type
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [`TreeKey#option`]).
    /// An iterator with an exact and trusted `size_hint()` can be obtained from
    /// this through [`NodeIter::exact_size()`].
    /// The maximum key depth may be selected independently of `Y` through the `D`
    /// const generic of [`NodeIter`].
    ///
    /// Potential [`Transcode`] targets:
    ///
    /// * [`crate::Path`]
    /// * [`crate::Indices`]
    /// * [`crate::Packed`]
    /// * [`crate::JsonPath`]
    /// * `()` (the unit)
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth = 1)]
    ///     bar: [u16; 2],
    /// };
    ///
    /// let paths = S::nodes::<Path<String, '/'>>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect::<Vec<_>>();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    ///
    /// let paths = S::nodes::<JsonPath<String>>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect::<Vec<_>>();
    /// assert_eq!(paths, [".foo", ".bar[0]", ".bar[1]"]);
    ///
    /// let indices = S::nodes::<Indices<[_; 2]>>()
    ///     .exact_size()
    ///     .map(|p| {
    ///         let (idx, node) = p.unwrap();
    ///         (idx.into_inner(), node.depth)
    ///     })
    ///     .collect::<Vec<_>>();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    ///
    /// let packed = S::nodes::<Packed>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_lsb().get())
    ///     .collect::<Vec<_>>();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    ///
    /// let nodes = S::nodes::<()>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().1)
    ///     .collect::<Vec<_>>();
    /// assert_eq!(nodes, [Node::leaf(1), Node::leaf(2), Node::leaf(2)]);
    /// ```
    fn nodes<N>() -> NodeIter<Self, Y, N>
    where
        N: Transcode + Default,
    {
        NodeIter::default()
    }
}

/// Access any node by keys.
///
/// This uses the `dyn Any` trait object.
///
/// ```
/// use core::any::Any;
/// use miniconf::{Indices, IntoKeys, JsonPath, TreeAny, TreeKey};
/// #[derive(TreeKey, TreeAny, Default)]
/// struct S {
///     foo: u32,
///     #[tree(depth = 1)]
///     bar: [u16; 2],
/// };
/// let mut s = S::default();
///
/// for node in S::nodes::<Indices<[_; 2]>>() {
///     let (key, node) = node.unwrap();
///     let a = s
///         .ref_any_by_key(key.into_iter().take(node.depth()).into_keys())
///         .unwrap();
///     assert!([0u32.type_id(), 0u16.type_id()].contains(&(&*a).type_id()));
/// }
///
/// let val: &mut u16 = s.mut_by_key(&JsonPath::from(".bar[1]")).unwrap();
/// *val = 3;
/// assert_eq!(s.bar[1], 3);
///
/// let val: &u16 = s.ref_by_key(&JsonPath::from(".bar[1]")).unwrap();
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
/// See also [`crate::json`] or `crate::postcard` for convenient functions using these traits.
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
    /// use miniconf::{IntoKeys, TreeKey, TreeSerialize};
    /// #[derive(TreeKey, TreeSerialize)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth = 1)]
    ///     bar: [u16; 2],
    /// };
    /// let s = S {
    ///     foo: 9,
    ///     bar: [11, 3],
    /// };
    /// let mut buf = [0u8; 10];
    /// let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    /// s.serialize_by_key(["bar", "0"].into_keys(), &mut ser)
    ///     .unwrap();
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
/// See also [`crate::json`] for a convenient helper functions using this trait.
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
    /// use miniconf::{IntoKeys, TreeDeserialize, TreeKey};
    /// #[derive(Default, TreeKey, TreeDeserialize)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth = 1)]
    ///     bar: [u16; 2],
    /// };
    /// let mut s = S::default();
    /// let mut de = serde_json_core::de::Deserializer::new(b"7", None);
    /// s.deserialize_by_key(["bar", "0"].into_keys(), &mut de)
    ///     .unwrap();
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

/// Shorthand for owned deserialization through [`TreeDeserialize`].
pub trait TreeDeserializeOwned<const Y: usize = 1>: for<'de> TreeDeserialize<'de, Y> {}
impl<T, const Y: usize> TreeDeserializeOwned<Y> for T where T: for<'de> TreeDeserialize<'de, Y> {}
