use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{IntoKeys, Keys, Schema, SerdeError, ValueError};

/// Traversal, iteration of keys in a tree.
///
/// See also the sub-traits [`TreeSerialize`], [`TreeDeserialize`], [`TreeAny`].
///
/// # Keys
///
/// There is a one-to-one relationship between nodes and keys.
/// The keys used to identify nodes support [`Keys`]/[`IntoKeys`]. They can be
/// obtained from other [`IntoKeys`] through [`Schema::transcode()`].
/// An iterator of keys for the nodes is available through [`Schema::nodes()`].
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
/// [`macro@crate::TreeSchema`], [`macro@crate::TreeSerialize`], [`macro@crate::TreeDeserialize`],
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
/// use miniconf::{Path, Tree, TreeSchema};
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(rename = "OTHER")]
///     a: f32,
/// };
/// let name = S::SCHEMA.transcode::<Path<String, '/'>>([0usize]).unwrap();
/// assert_eq!(name.0.as_str(), "/OTHER");
/// ```
///
/// ## Skip
///
/// Named fields/variants may be omitted from the derived `Tree` trait implementations using the
/// `skip` attribute.
/// Note that for tuple structs skipping is only supported for terminal fields:
///
/// ```
/// use miniconf::{Tree};
/// #[derive(Tree)]
/// struct S(i32, #[tree(skip)] ());
/// ```
///
/// ```compile_fail
/// use miniconf::{Tree};
/// #[derive(Tree)]
/// struct S(#[tree(skip)] (), i32);
/// ```
///
/// ## Type
///
/// The type to use when accessing the field/variant through `TreeDeserialize::probe`
/// can be overridden using the `typ` derive macro attribute (`#[tree(typ="[f32; 4]")]`).
///
/// ## Implementation overrides
///
/// `#[tree(with=path)]`
///
/// This overrides the calls to the child node/variant traits using pub functions
/// and constants in the module at the given path:
/// (`SCHEMA`, `serialize_by_key`, `deserialize_by_key`, `probe_by_key`,
/// `ref_any_by_key`, `mut_any_by_key`).
///
/// Also use this to relax bounds and deny operations.
/// ```
/// # use miniconf::{SerdeError, Tree, Keys, ValueError, TreeDeserialize};
/// # use serde::Deserializer;
/// #[derive(Tree, Default)]
/// struct S {
///     #[tree(with=check)]
///     b: f32,
/// }
/// mod check {
///     use miniconf::{SerdeError, Deserializer, TreeDeserialize, ValueError, Keys};
///     pub use miniconf::leaf::{SCHEMA, serialize_by_key, probe_by_key, ref_any_by_key, mut_any_by_key};
///
///     pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
///         value: &mut f32,
///         keys: impl Keys,
///         de: D
///     ) -> Result<(), SerdeError<D::Error>> {
///         let mut new = *value;
///         new.deserialize_by_key(keys, de)?;
///         if new < 0.0 {
///             Err(ValueError::Access("fail").into())
///         } else {
///             *value = new;
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
/// iterators. The `TreeSchema` behavior of an [`Option`] is such that the `None` variant makes the
/// corresponding part of the tree inaccessible at run-time. It will still be iterated over (e.g.
/// by [`Schema::nodes()`]) but attempts to access it (e.g. [`TreeSerialize::serialize_by_key()`],
/// [`TreeDeserialize::deserialize_by_key()`], [`TreeAny::ref_any_by_key()`], or
/// [`TreeAny::mut_any_by_key()`]) return the special [`ValueError::Absent`].
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
pub trait TreeSchema {
    /// Schema for this tree level
    // Reference for Option<T> to copy T::SCHEMA
    const SCHEMA: &Schema;
}

/// Access any node by keys.
///
/// This uses the `dyn Any` trait object.
///
/// ```
/// use core::any::Any;
/// use miniconf::{Indices, IntoKeys, JsonPath, TreeAny, TreeSchema};
/// #[derive(TreeSchema, TreeAny, Default)]
/// struct S {
///     foo: u32,
///     bar: [u16; 2],
/// };
/// let mut s = S::default();
///
/// for key in S::SCHEMA.nodes::<Indices<[_; 2]>, 2>() {
///     let a = s.ref_any_by_key(key.unwrap().into_keys()).unwrap();
///     assert!([0u32.type_id(), 0u16.type_id()].contains(&(&*a).type_id()));
/// }
///
/// let val: &mut u16 = s.mut_by_key(&JsonPath(".bar[1]")).unwrap();
/// *val = 3;
/// assert_eq!(s.bar[1], 3);
///
/// let val: &u16 = s.ref_by_key(&JsonPath(".bar[1]")).unwrap();
/// assert_eq!(*val, 3);
/// ```
pub trait TreeAny: TreeSchema {
    /// Obtain a reference to a `dyn Any` trait object for a leaf node.
    fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError>;

    /// Obtain a mutable reference to a `dyn Any` trait object for a leaf node.
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError>;

    /// Obtain a reference to a leaf of known type by key.
    #[inline]
    fn ref_by_key<T: Any>(&self, keys: impl IntoKeys) -> Result<&T, ValueError> {
        self.ref_any_by_key(keys.into_keys())?
            .downcast_ref()
            .ok_or(ValueError::Access("Incorrect type"))
    }

    /// Obtain a mutable reference to a leaf of known type by key.
    #[inline]
    fn mut_by_key<T: Any>(&mut self, keys: impl IntoKeys) -> Result<&mut T, ValueError> {
        self.mut_any_by_key(keys.into_keys())?
            .downcast_mut()
            .ok_or(ValueError::Access("Incorrect type"))
    }
}

/// Serialize a leaf node by its keys.
///
/// See also [`crate::json_core`] or `crate::postcard` for convenient wrappers using this trait.
///
/// # Derive macro
///
/// See [`macro@crate::TreeSerialize`].
/// The derive macro attributes are described in the [`TreeSchema`] trait.
pub trait TreeSerialize: TreeSchema {
    /// Serialize a node by keys.
    ///
    /// ```
    /// # #[cfg(feature = "json-core")] {
    /// use miniconf::{IntoKeys, TreeSchema, TreeSerialize};
    /// #[derive(TreeSchema, TreeSerialize)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// };
    /// let s = S {
    ///     foo: 9,
    ///     bar: [11, 3],
    /// };
    /// let mut buf = [0u8; 10];
    /// let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    /// s.serialize_by_key(["bar", "0"].into_keys(), &mut ser).unwrap();
    /// let len = ser.end();
    /// assert_eq!(&buf[..len], b"11");
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: A `Keys` identifying the node.
    /// * `ser`: A `Serializer` to to serialize the value.
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>>;
}

/// Deserialize a leaf node by its keys.
///
/// See also [`crate::json_core`] or `crate::postcard` for convenient wrappers using this trait.
///
/// # Derive macro
///
/// See [`macro@crate::TreeDeserialize`].
/// The derive macro attributes are described in the [`TreeSchema`] trait.
pub trait TreeDeserialize<'de>: TreeSchema {
    /// Deserialize a leaf node by its keys.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{IntoKeys, TreeDeserialize, TreeSchema};
    /// #[derive(Default, TreeSchema, TreeDeserialize)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
    /// };
    /// let mut s = S::default();
    /// let mut de = serde_json::de::Deserializer::from_slice(b"7");
    /// s.deserialize_by_key(["bar", "0"].into_keys(), &mut de).unwrap();
    /// de.end().unwrap();
    /// assert_eq!(s.bar[0], 7);
    /// # }
    /// ```
    ///
    /// # Args
    /// * `keys`: A `Keys` identifying the node.
    /// * `de`: A `Deserializer` to deserialize the value.
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>>;

    /// Blind deserialize a leaf node by its keys.
    ///
    /// This method should succeed at least in those cases where
    /// `deserialize_by_key()` succeeds.
    ///
    /// ```
    /// # #[cfg(feature = "derive")] {
    /// use miniconf::{IntoKeys, TreeDeserialize, TreeSchema};
    /// #[derive(Default, TreeSchema, TreeDeserialize)]
    /// struct S {
    ///     foo: u32,
    ///     bar: [u16; 2],
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
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>>;
}

/// Shorthand for owned deserialization through [`TreeDeserialize`].
pub trait TreeDeserializeOwned: for<'de> TreeDeserialize<'de> {}
impl<T> TreeDeserializeOwned for T where T: for<'de> TreeDeserialize<'de> {}

// Blanket impls for refs and muts

impl<T: TreeSchema + ?Sized> TreeSchema for &T {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSchema + ?Sized> TreeSchema for &mut T {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSerialize + ?Sized> TreeSerialize for &T {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        (**self).serialize_by_key(keys, ser)
    }
}

impl<T: TreeSerialize + ?Sized> TreeSerialize for &mut T {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        (**self).serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de> + ?Sized> TreeDeserialize<'de> for &mut T {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        (**self).deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        T::probe_by_key(keys, de)
    }
}

impl<T: TreeAny + ?Sized> TreeAny for &mut T {
    #[inline]
    fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
        (**self).ref_any_by_key(keys)
    }

    #[inline]
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        (**self).mut_any_by_key(keys)
    }
}
