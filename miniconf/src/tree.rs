use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{Error, IntoKeys, Keys, Node, NodeIter, Transcode, Traversal, Walk};

/// Traversal, iteration of keys in a tree.
///
/// See also the sub-traits [`TreeSerialize`], [`TreeDeserialize`], [`TreeAny`].
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
/// ## Rename
///
/// The key for named struct fields may be changed from the default field ident using the `rename`
/// derive macro attribute.
///
/// ```
/// use miniconf::{Leaf, Path, Tree, TreeKey};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(rename = "OTHER")]
///     a: Leaf<f32>,
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
/// use miniconf::{Tree, Leaf};
/// #[derive(Tree)]
/// struct S(#[tree(skip)] (), Leaf<i32>);
/// ```
///
/// ```
/// use miniconf::{Leaf, Tree};
/// #[derive(Tree)]
/// struct S(Leaf<i32>, #[tree(skip)] ());
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
/// use miniconf::{Error, Leaf, Tree};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(validate=non_leaf)]
///     b: [Leaf<f32>; 2],
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
/// TODO
///
/// ```
/// use miniconf::{Leaf, Metadata, TreeKey};
/// #[derive(TreeKey)]
/// struct S<T> {
///     a: [Option<T>; 2],
/// };
/// // This works as [Leaf<u32>; N] and Leaf<u32> implement TreeKey:
/// S::<[Leaf<u32>; 5]>::traverse_all::<Metadata>();
/// // This does not compile as u32 does not implement TreeKey:
/// // S::<u32>::traverse_all::<Metadata>();
/// ```
///
/// # Array
///
/// Blanket implementations of the `TreeKey` traits are provided for homogeneous arrays
/// [`[T; N]`](core::array).
///
/// # Option
///
/// Blanket implementations of the `TreeKey` traits are provided for [`Option<T>`].
///
/// These implementations do not alter the path hierarchy and do not consume any items from the `keys`
/// iterators. The `TreeKey` behavior of an [`Option`] is such that the `None` variant makes the
/// corresponding part of the tree inaccessible at run-time. It will still be iterated over (e.g.
/// by [`TreeKey::nodes()`]) but attempts to access it (e.g. [`TreeSerialize::serialize_by_key()`],
/// [`TreeDeserialize::deserialize_by_key()`], [`TreeAny::ref_any_by_key()`], or
/// [`TreeAny::mut_any_by_key()`]) return the special [`Traversal::Absent`].
///
/// # Examples
///
/// See the [`crate`] documentation for a longer example showing how the traits and the derive
/// macros work.
pub trait TreeKey {
    /// Walk metadata about all paths.
    ///
    /// ```
    /// use miniconf::{Leaf, Metadata, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
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
    /// use miniconf::{IntoKeys, Leaf, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
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
    ///
    /// # Design note
    /// Writing this to return an iterator instead of using a callback
    /// would have worse performance (O(n^2) instead of O(n) for matching)
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
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
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 5],
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
        let node = target.transcode::<Self, _>(keys)?;
        Ok((target, node))
    }

    /// Return an iterator over nodes of a given type
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [`TreeKey#option`]).
    /// An iterator with an exact and trusted `size_hint()` can be obtained from
    /// this through [`NodeIter::exact_size()`].
    /// The `D`
    /// const generic of [`NodeIter`] is the maximum key depth.
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
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    ///
    /// let paths = S::nodes::<Path<String, '/'>, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect::<Vec<_>>();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    ///
    /// let paths = S::nodes::<JsonPath<String>, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect::<Vec<_>>();
    /// assert_eq!(paths, [".foo", ".bar[0]", ".bar[1]"]);
    ///
    /// let indices = S::nodes::<Indices<[_; 2]>, 2>()
    ///     .exact_size()
    ///     .map(|p| {
    ///         let (idx, node) = p.unwrap();
    ///         (idx.into_inner(), node.depth)
    ///     })
    ///     .collect::<Vec<_>>();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    ///
    /// let packed = S::nodes::<Packed, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_lsb().get())
    ///     .collect::<Vec<_>>();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    ///
    /// let nodes = S::nodes::<(), 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().1)
    ///     .collect::<Vec<_>>();
    /// assert_eq!(nodes, [Node::leaf(1), Node::leaf(2), Node::leaf(2)]);
    /// ```
    fn nodes<N, const D: usize>() -> NodeIter<Self, N, D>
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
/// use miniconf::{Indices, IntoKeys, JsonPath, Leaf, TreeAny, TreeKey};
/// #[derive(TreeKey, TreeAny, Default)]
/// struct S {
///     foo: Leaf<u32>,
///     bar: [Leaf<u16>; 2],
/// };
/// let mut s = S::default();
///
/// for node in S::nodes::<Indices<[_; 2]>, 2>() {
///     let (key, node) = node.unwrap();
///     let a = s
///         .ref_any_by_key(key.into_iter().take(node.depth()).into_keys())
///         .unwrap();
///     assert!([0u32.type_id(), 0u16.type_id()].contains(&(&*a).type_id()));
/// }
///
/// let val: &mut u16 = s.mut_by_key(&JsonPath::from(".bar[1]")).unwrap();
/// *val = 3;
/// assert_eq!(*s.bar[1], 3);
///
/// let val: &u16 = s.ref_by_key(&JsonPath::from(".bar[1]")).unwrap();
/// assert_eq!(*val, 3);
/// ```
pub trait TreeAny: TreeKey {
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
pub trait TreeSerialize: TreeKey {
    /// Serialize a node by keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// use miniconf::{IntoKeys, Leaf, TreeKey, TreeSerialize};
    /// #[derive(TreeKey, TreeSerialize)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    /// let s = S {
    ///     foo: 9.into(),
    ///     bar: [11.into(), 3.into()],
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
pub trait TreeDeserialize<'de>: TreeKey {
    /// Deserialize a leaf node by its keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// use miniconf::{IntoKeys, Leaf, TreeDeserialize, TreeKey};
    /// #[derive(Default, TreeKey, TreeDeserialize)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    /// let mut s = S::default();
    /// let mut de = serde_json_core::de::Deserializer::new(b"7", None);
    /// s.deserialize_by_key(["bar", "0"].into_keys(), &mut de)
    ///     .unwrap();
    /// de.end().unwrap();
    /// assert_eq!(*s.bar[0], 7);
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
pub trait TreeDeserializeOwned: for<'de> TreeDeserialize<'de> {}
impl<T> TreeDeserializeOwned for T where T: for<'de> TreeDeserialize<'de> {}
