use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{Error, IntoKeys, Keys, Schema, Traversal};

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
/// The type to use when accessing the field/variant through `TreeKey`/`TreeDeserialize::probe`
/// can be overridden using the `typ` derive macro attribute (`#[tree(typ="[f32; 4]")]`).
///
/// ## Deny
///
/// `#[tree(deny(operation="message", ...))]`
///
/// This returns `Err(`[`Traversal::Access`]`)` for the respective operation
/// (`traverse`, `serialize`, `deserialize`, `probe`, `ref_any`, `mut_any`) on a
/// field/variant and suppresses the respective traits bounds on type paramters
/// of the struct/enum.
///
/// ## Implementation overrides
///
/// `#[tree(with(operation=expr, ...))]`
///
/// This overrides the call to the child node/variant trait for the given `operation`
/// (`traverse`, `traverse_all`, `serialize`, `deserialize`, `probe`, `ref_any`, `mut_any`).
/// `expr` should be a method on `self` (not the field!) or `value`
/// (associated function for `traverse`, `traverse_all` and `probe`)
/// taking the arguments of the respective trait's method.
///
/// ```
/// # use miniconf::{Error, Leaf, Tree, Keys, Traversal, TreeDeserialize};
/// # use serde::Deserializer;
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(with(deserialize=self.check))]
///     b: Leaf<f32>,
/// };
/// impl S {
///     fn check<'de, K: Keys, D: Deserializer<'de>>(&mut self, keys: K, de: D) -> Result<(), Error<D::Error>> {
///         let old = *self.b;
///         self.b.deserialize_by_key(keys, de)?;
///         if *self.b < 0.0 {
///             *self.b = old;
///             Err(Traversal::Access(0, "fail").into())
///         } else {
///             Ok(())
///         }
///     }
/// }
/// ```
///
/// ### `defer`
///
/// The `defer` attribute is a shorthand for `with()` that defers
/// child trait implementations to a given expression.
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
    /// Schema for this tree level
    const SCHEMA: &'static Schema;
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
    #[inline]
    fn ref_by_key<T: Any, K: IntoKeys>(&self, keys: K) -> Result<&T, Traversal> {
        self.ref_any_by_key(keys.into_keys())?
            .downcast_ref()
            .ok_or(Traversal::Access(0, "Incorrect type"))
    }

    /// Obtain a mutable reference to a leaf of known type by key.
    #[inline]
    fn mut_by_key<T: Any, K: IntoKeys>(&mut self, keys: K) -> Result<&mut T, Traversal> {
        self.mut_any_by_key(keys.into_keys())?
            .downcast_mut()
            .ok_or(Traversal::Access(0, "Incorrect type"))
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
pub trait TreeDeserialize<'de>: TreeKey {
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
    /// This method should succeed at least in those cases where
    /// `deserialize_by_key()` succeeds.
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
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeKey> TreeKey for &mut T {
    const SCHEMA: &'static Schema = T::SCHEMA;
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
