use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{IntoKeys, Keys, Schema, SerdeError, ValueError};

pub trait TreeSchema {
    /// Schema for this tree level
    // Reference for Option<T> to copy T::SCHEMA
    const SCHEMA: &'static Schema;
}

/// Access any node by keys.
///
/// This uses the `dyn Any` trait object.
///
/// ```
/// use core::any::Any;
/// use miniconf::{Indices, IntoKeys, JsonPath, Leaf, TreeAny, TreeSchema};
/// #[derive(TreeSchema, TreeAny, Default)]
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
/// See also [`crate::json`] or `crate::postcard` for convenient wrappers using this trait.
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
    /// use miniconf::{IntoKeys, Leaf, TreeSchema, TreeSerialize};
    /// #[derive(TreeSchema, TreeSerialize)]
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
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>>;
}

/// Deserialize a leaf node by its keys.
///
/// See also [`crate::json`] or `crate::postcard` for convenient wrappers using this trait.
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
    /// use miniconf::{IntoKeys, Leaf, TreeDeserialize, TreeSchema};
    /// #[derive(Default, TreeSchema, TreeDeserialize)]
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
    /// use miniconf::{IntoKeys, Leaf, TreeDeserialize, TreeSchema};
    /// #[derive(Default, TreeSchema, TreeDeserialize)]
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
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>>;
}

/// Shorthand for owned deserialization through [`TreeDeserialize`].
pub trait TreeDeserializeOwned: for<'de> TreeDeserialize<'de> {}
impl<T> TreeDeserializeOwned for T where T: for<'de> TreeDeserialize<'de> {}

// Blanket impls for refs and muts

impl<T: TreeSchema> TreeSchema for &T {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSchema> TreeSchema for &mut T {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSerialize> TreeSerialize for &T {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        (**self).serialize_by_key(keys, ser)
    }
}

impl<T: TreeSerialize> TreeSerialize for &mut T {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        (**self).serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &mut T {
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

impl<T: TreeAny> TreeAny for &mut T {
    #[inline]
    fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
        (**self).ref_any_by_key(keys)
    }

    #[inline]
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        (**self).mut_any_by_key(keys)
    }
}
