use core::{
    any::Any,
    fmt::Display,
    num::NonZero,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk};

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

impl<T: ?Sized> TreeKey for Leaf<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        Ok(W::leaf())
    }

    #[inline]
    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        keys.finalize()?;
        Ok(0)
    }
}

impl<T: Serialize + ?Sized> TreeSerialize for Leaf<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        keys.finalize()?;
        self.0.serialize(ser).map_err(|err| Error::Inner(0, err))
    }
}

impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Leaf<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        self.0 = T::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        Ok(())
    }

    #[inline]
    fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        T::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        Ok(())
    }
}

impl<T: Any> TreeAny for Leaf<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Ok(&self.0)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
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
///     #[tree(typ="En", defer=*self.e)]
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

impl<T: ?Sized> TreeKey for StrLeaf<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        Ok(W::leaf())
    }

    #[inline]
    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        keys.finalize()?;
        Ok(0)
    }
}

impl<T: AsRef<str> + ?Sized> TreeSerialize for StrLeaf<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        keys.finalize()?;
        let name = self.0.as_ref();
        name.serialize(ser).map_err(|err| Error::Inner(0, err))
    }
}

impl<'de, T: TryFrom<&'de str>> TreeDeserialize<'de> for StrLeaf<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        self.0 = T::try_from(name).or(Err(Traversal::Invalid(0, "Could not convert from str")))?;
        Ok(())
    }

    #[inline]
    fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        T::try_from(name).or(Err(Traversal::Invalid(0, "Could not convert from str")))?;
        Ok(())
    }
}

impl<T> TreeAny for StrLeaf<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "No Any access for StrLeaf"))
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "No Any access for StrLeaf"))
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

impl<T: ?Sized> TreeKey for Deny<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        Ok(W::leaf())
    }

    #[inline]
    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        keys.finalize()?;
        Ok(0)
    }
}

impl<T: ?Sized> TreeSerialize for Deny<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, _ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "Denied").into())
    }
}

impl<'de, T: ?Sized> TreeDeserialize<'de> for Deny<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, _de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "Denied").into())
    }

    #[inline]
    fn probe_by_key<K, D>(mut keys: K, _de: D) -> Result<(), Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "Denied").into())
    }
}

impl<T> TreeAny for Deny<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "Denied"))
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Err(Traversal::Access(0, "Denied"))
    }
}
