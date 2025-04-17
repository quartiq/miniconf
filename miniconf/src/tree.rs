use core::{any::Any, num::NonZero};

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
/// * [`crate::Packed`] is a bit-packed compact compressed notation of
///   hierarchical compound indices.
/// * See the `scpi` example for how to implement case-insensitive, relative, and abbreviated/partial
///   matches.
///
/// # Derive macros
///
/// Derive macros to automatically implement the correct traits on a struct or enum are available through
/// [`macro@crate::TreeKey`], [`macro@crate::TreeSerialize`], [`macro@crate::TreeDeserialize`],
/// and [`macro@crate::TreeAny`].
/// A shorthand derive macro that derives all four trait implementations is also available at
/// [`macro@crate::Tree`].
///
/// The derive macros support per-field/per-variant attributes to control the derived trait implementations.
///
/// ## Rename
///
/// The key for named struct fields or enum variants may be changed from the default field ident using
/// the `rename` derive macro attribute.
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
/// Named fields/variants may be omitted from the derived `Tree` trait implementations using the
/// `skip` attribute.
/// Note that for tuple structs skipping is only supported for terminal fields:
///
/// ```
/// use miniconf::{Leaf, Tree};
/// #[derive(Tree)]
/// struct S(Leaf<i32>, #[tree(skip)] ());
/// ```
///
/// ```compile_fail
/// use miniconf::{Tree, Leaf};
/// #[derive(Tree)]
/// struct S(#[tree(skip)] (), Leaf<i32>);
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
/// validation or support remote types (e.g. `#[tree(get_mut=func())]`)
///
/// ### `get`
///
/// The getter is called during `serialize_by_key()` before leaf serialization and
/// during `ref_any_by_key()`. Its signature is `fn() -> Result<&T, &'static str>`.
/// The default getter is `Ok(&self.field)`. `&self` is in scope and can be used.
/// If a getter returns an error message `Err(&str)` the serialization/traversal
/// is not performed, further getters at greater depth are not invoked
/// and [`Traversal::Access`] is returned.
///
/// ### `get_mut`
///
/// `get_mut` is invoked during `mut_any_by_key()` and
/// during `deserialize_by_key()` before deserialization while traversing down to
/// the leaf node.
/// The signature is `fn() -> Result<&mut T, &str>`. `&mut self` is in scope and
/// can be used/mutated.
/// The default `get_mut` is `Ok(&mut self.field)`.
/// If `get_mut` returns an `Err` [`Traversal::Access`] will be returned.
///
/// ### `validate`
///
/// `validate` is called after the successful update of the leaf field
/// during upward traversal.
/// The `validate` signature is `fn(depth: usize) ->
/// Result<usize, &'static str>`. `&mut self` is in scope and can be used/mutated.
/// If a validate callback returns `Err()`, the leaf value already **has been**
/// updated and [`Traversal::Invalid`] is returned from `deserialize_by_key()`.
///
/// ```
/// use miniconf::{Error, Leaf, Tree};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(validate=self.non_leaf)]
///     b: [Leaf<f32>; 2],
/// };
/// impl S {
///     fn non_leaf(&mut self) -> Result<(), &'static str> {
///         Err("fail")
///     }
/// }
/// ```
///
/// ### `defer`
///
/// The `defer` attribute is a shorthand for `get`+`get_mut` of the same owned value.
///
/// # Array
///
/// Blanket implementations of the `Tree*` traits are provided for homogeneous arrays
/// [`[T; N]`](core::array).
///
/// # Option
///
/// Blanket implementations of the `Tree*` traits are provided for [`Option<T>`].
///
/// These implementations do not alter the path hierarchy and do not consume any items from the `keys`
/// iterators. The `TreeKey` behavior of an [`Option`] is such that the `None` variant makes the
/// corresponding part of the tree inaccessible at run-time. It will still be iterated over (e.g.
/// by [`TreeKey::nodes()`]) but attempts to access it (e.g. [`TreeSerialize::serialize_by_key()`],
/// [`TreeDeserialize::deserialize_by_key()`], [`TreeAny::ref_any_by_key()`], or
/// [`TreeAny::mut_any_by_key()`]) return the special [`Traversal::Absent`].
///
/// This is the same behavior as for other `enums` that have the `Tree*` traits derived.
///
/// # Tuples
///
/// Blanket impementations for the `Tree*` traits are provided for heterogeneous tuples `(T0, T1, ...)`
/// up to length eight.
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
    /// let m: Metadata = S::traverse_all().unwrap();
    /// assert_eq!((m.max_depth, m.max_length, m.count.get()), (2, 4, 3));
    /// ```
    fn traverse_all<W: Walk>() -> Result<W, W::Error>;

    /// Traverse from the root to a leaf and call a function for each node.
    ///
    /// If a leaf is found early (`keys` being longer than required)
    /// `Err(Traversal(TooLong(depth)))` is returned.
    /// If `keys` is exhausted before reaching a leaf node,
    /// `Err(Traversal(TooShort(depth)))` is returned.
    /// `Traversal::Access/Invalid/Absent/Finalization` are never returned.
    ///
    /// ```
    /// use miniconf::{IntoKeys, Leaf, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    /// let mut ret = [(1, Some("bar"), 2), (0, None, 2)].into_iter();
    /// let func = |index, name, len: core::num::NonZero<usize>| -> Result<(), ()> {
    ///     assert_eq!(ret.next().unwrap(), (index, name, len.get()));
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
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>;

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
    /// ```
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 5],
    /// };
    ///
    /// let idx = [1, 1];
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
    #[inline]
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
    /// The `D` const generic of [`NodeIter`] is the maximum key depth.
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    ///
    /// let paths: Vec<_> = S::nodes::<Path<String, '/'>, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    ///
    /// let paths: Vec<_> = S::nodes::<JsonPath<String>, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect();
    /// assert_eq!(paths, [".foo", ".bar[0]", ".bar[1]"]);
    ///
    /// let indices: Vec<_> = S::nodes::<Indices<[_; 2]>, 2>()
    ///     .exact_size()
    ///     .map(|p| {
    ///         let (idx, node) = p.unwrap();
    ///         (idx.into_inner(), node.depth)
    ///     })
    ///     .collect();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    ///
    /// let packed: Vec<_> = S::nodes::<Packed, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_lsb().get())
    ///     .collect();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    ///
    /// let nodes: Vec<_> = S::nodes::<(), 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().1)
    ///     .collect();
    /// assert_eq!(nodes, [Node::leaf(1), Node::leaf(2), Node::leaf(2)]);
    /// ```
    #[inline]
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
pub trait TreeAny {
    /// Obtain a reference to a `dyn Any` trait object for a leaf node.
    fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys;

    /// Obtain a mutable reference to a `dyn Any` trait object for a leaf node.
    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys;

    /// Obtain a reference to a leaf of known type by key.
    #[inline]
    fn ref_by_key<T: Any, K: IntoKeys>(&self, keys: K) -> Result<&T, Traversal> {
        self.ref_any_by_key(keys.into_keys())?
            .downcast_ref()
            .ok_or(Traversal::Invalid(0, "Incorrect type"))
    }

    /// Obtain a mutable reference to a leaf of known type by key.
    #[inline]
    fn mut_by_key<T: Any, K: IntoKeys>(&mut self, keys: K) -> Result<&mut T, Traversal> {
        self.mut_any_by_key(keys.into_keys())?
            .downcast_mut()
            .ok_or(Traversal::Invalid(0, "Incorrect type"))
    }
}

/// Serialize a leaf node by its keys.
///
/// See also [`crate::json`] or `crate::postcard` for convenient wrappers using this trait.
///
/// # Derive macro
///
/// See [`macro@crate::TreeSerialize`].
/// The derive macro attributes are described in the [`TreeKey`] trait.
pub trait TreeSerialize {
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
    /// * `keys`: A `Keys` identifying the node.
    /// * `ser`: A `Serializer` to to serialize the value.
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        K: Keys,
        S: Serializer;
}

/// Deserialize a leaf node by its keys.
///
/// See also [`crate::json`] or `crate::postcard` for convenient wrappers using this trait.
///
/// # Derive macro
///
/// See [`macro@crate::TreeDeserialize`].
/// The derive macro attributes are described in the [`TreeKey`] trait.
pub trait TreeDeserialize<'de> {
    /// Deserialize a leaf node by its keys.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{IntoKeys, Leaf, TreeDeserialize, TreeKey};
    /// #[derive(Default, TreeKey, TreeDeserialize)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    /// let mut s = S::default();
    /// let mut de = serde_json::de::Deserializer::from_slice(b"7");
    /// s.deserialize_by_key(["bar", "0"].into_keys(), &mut de)
    ///     .unwrap();
    /// de.end().unwrap();
    /// assert_eq!(*s.bar[0], 7);
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: A `Keys` identifying the node.
    /// * `de`: A `Deserializer` to deserialize the value.
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>;

    /// Blind deserialize a leaf node by its keys.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{IntoKeys, Leaf, TreeDeserialize, TreeKey};
    /// #[derive(Default, TreeKey, TreeDeserialize)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    /// let mut de = serde_json::de::Deserializer::from_slice(b"7");
    /// S::probe_by_key(["bar", "0"].into_keys(), &mut de)
    ///     .unwrap();
    /// de.end().unwrap();
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: A `Keys` identifying the node.
    /// * `de`: A `Deserializer` to deserialize the value.
    fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>;
}

/// Shorthand for owned deserialization through [`TreeDeserialize`].
pub trait TreeDeserializeOwned: for<'de> TreeDeserialize<'de> {}
impl<T> TreeDeserializeOwned for T where T: for<'de> TreeDeserialize<'de> {}

// Blanket impls for refs and muts

impl<T: TreeKey> TreeKey for &T {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    #[inline]
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: TreeKey> TreeKey for &mut T {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    #[inline]
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: TreeSerialize> TreeSerialize for &T {
    #[inline]
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        (**self).serialize_by_key(keys, ser)
    }
}

impl<T: TreeSerialize> TreeSerialize for &mut T {
    #[inline]
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        (**self).serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &mut T {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        (**self).deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        T::probe_by_key(keys, de)
    }
}

impl<T: TreeAny> TreeAny for &mut T {
    #[inline]
    fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        (**self).ref_any_by_key(keys)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        (**self).mut_any_by_key(keys)
    }
}
