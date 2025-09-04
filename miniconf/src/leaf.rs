use core::{
    any::Any,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Keys, Schema, SerdeError, TreeAny, TreeDeserialize, TreeSchema, TreeSerialize, ValueError,
};

/// `Serialize`/`Deserialize`/`Any` leaf
///
/// This wraps [`Serialize`], [`Deserialize`], and [`Any`] into `Tree` a leaf node.
///
/// ```
/// use miniconf::{json, Leaf, Tree};
/// let mut s = Leaf(0);
/// json::set(&mut s, "", b"7").unwrap();
/// assert!(matches!(*s, 7));
/// ```
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Leaf<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for Leaf<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for Leaf<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Leaf<T> {
    /// Extract just the inner
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Leaf<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: Display> Display for Leaf<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: ?Sized> TreeSchema for Leaf<T> {
    const SCHEMA: &'static Schema = &Schema::LEAF;
}

impl<T: Serialize + ?Sized> TreeSerialize for Leaf<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        self.0.serialize(ser).map_err(SerdeError::Inner)
    }
}

impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Leaf<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        self.0 = T::deserialize(de).map_err(SerdeError::Inner)?;
        Ok(())
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        T::deserialize(de).map_err(SerdeError::Inner)?;
        Ok(())
    }
}

impl<T: Any> TreeAny for Leaf<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        keys.finalize()?;
        Ok(&self.0)
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        keys.finalize()?;
        Ok(&mut self.0)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

/// `TryFrom<&str>`/`AsRef<str>` leaf
///
/// This wraps [`TryFrom<&str>`] and [`AsRef<str>`] into a `Tree*` leaf.
/// [`TreeAny`] is implemented but denied access at runtime.
/// It is especially useful to support enum variant switching using `strum`.
/// Inner enum variant field access can be implemented using `defer`.
///
/// ```
/// use miniconf::{json, Leaf, StrLeaf, Tree};
/// #[derive(Tree, strum::AsRefStr, strum::EnumString)]
/// enum En {
///     A(Leaf<i32>),
///     B(Leaf<f32>),
/// }
/// #[derive(Tree)]
/// struct S {
///     e: StrLeaf<En>,
///     #[tree(typ="En", defer=(*self.e))]
///     t: (),
/// }
/// let mut s = S {
///     e: StrLeaf(En::A(9.into())),
///     t: (),
/// };
/// json::set(&mut s, "/e", b"\"B\"").unwrap();
/// json::set(&mut s, "/t/B", b"1.2").unwrap();
/// assert!(matches!(*s.e, En::B(Leaf(1.2))));
/// ```
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct StrLeaf<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for StrLeaf<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for StrLeaf<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> StrLeaf<T> {
    /// Extract just the inner
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for StrLeaf<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: ?Sized> TreeSchema for StrLeaf<T> {
    const SCHEMA: &'static Schema = &Schema::LEAF;
}

impl<T: AsRef<str> + ?Sized> TreeSerialize for StrLeaf<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        let name = self.0.as_ref();
        name.serialize(ser).map_err(SerdeError::Inner)
    }
}

impl<'de, T: TryFrom<&'de str>> TreeDeserialize<'de> for StrLeaf<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(SerdeError::Inner)?;
        self.0 = T::try_from(name).or(Err(ValueError::Access("Could not convert from str")))?;
        Ok(())
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(SerdeError::Inner)?;
        T::try_from(name).or(Err(ValueError::Access("Could not convert from str")))?;
        Ok(())
    }
}

impl<T> TreeAny for StrLeaf<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        keys.finalize()?;
        Err(ValueError::Access("No Any access for StrLeaf"))
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        keys.finalize()?;
        Err(ValueError::Access("No Any access for StrLeaf"))
    }
}

impl<T: Display> Display for StrLeaf<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// Deny any value access
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Deny<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for Deny<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for Deny<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Deny<T> {
    /// Extract just the inner
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Deny<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: TreeSchema + ?Sized> TreeSchema for Deny<T> {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSchema + ?Sized> TreeSerialize for Deny<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        _keys: impl Keys,
        _ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        Err(ValueError::Access("Denied").into())
    }
}

impl<'de, T: TreeSchema + ?Sized> TreeDeserialize<'de> for Deny<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        _keys: impl Keys,
        _de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        Err(ValueError::Access("Denied").into())
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        _keys: impl Keys,
        _de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        Err(ValueError::Access("Denied").into())
    }
}

impl<T: TreeSchema + ?Sized> TreeAny for Deny<T> {
    #[inline]
    fn ref_any_by_key(&self, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
        Err(ValueError::Access("Denied"))
    }

    #[inline]
    fn mut_any_by_key(&mut self, _keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        Err(ValueError::Access("Denied"))
    }
}
