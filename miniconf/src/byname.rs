use core::{
    any::Any,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk};

/// `TryFrom<&str>`/`AsRef<str>` set-by-name
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct StrLeaf<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for StrLeaf<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for StrLeaf<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> StrLeaf<T> {
    /// Extract just the inner
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for StrLeaf<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: ?Sized> TreeKey for StrLeaf<T> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        Ok(W::leaf())
    }

    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        keys.finalize()?;
        Ok(0)
    }
}

impl<T: AsRef<str> + ?Sized> TreeSerialize for StrLeaf<T> {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        keys.finalize()?;
        let name = self.0.as_ref();
        name.serialize(ser).map_err(|err| Error::Inner(0, err))?;
        Ok(0)
    }
}

impl<'de, T: TryFrom<&'de str>> TreeDeserialize<'de> for StrLeaf<T> {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        self.0 = T::try_from(name).or(Err(Traversal::Invalid(0, "Invalid name")))?;
        Ok(0)
    }
}

impl<T> TreeAny for StrLeaf<T> {
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Err(Traversal::Access(1, "No Any access for StrLeaf"))
    }

    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Err(Traversal::Access(1, "No Any access for StrLeaf"))
    }
}
