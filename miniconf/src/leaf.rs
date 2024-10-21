use core::{
    any::Any,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk};

/// Transparent leaf marker newtype
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Leaf<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for Leaf<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for Leaf<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Leaf<T> {
    /// Extract just the inner
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> TreeKey for Leaf<T> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        Ok(W::leaf())
    }

    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        if !keys.finalize() {
            Err(Traversal::TooLong(0).into())
        } else {
            Ok(0)
        }
    }
}

impl<T: Serialize> TreeSerialize for Leaf<T> {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        if !keys.finalize() {
            Err(Traversal::TooLong(0))?;
        }
        self.serialize(ser).map_err(|err| Error::Inner(0, err))?;
        Ok(0)
    }
}

impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Leaf<T> {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        if !keys.finalize() {
            Err(Traversal::TooLong(0))?;
        }
        *self = Self::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        Ok(0)
    }
}

impl<T: Any> TreeAny for Leaf<T> {
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        if !keys.finalize() {
            Err(Traversal::TooLong(0))?;
        }
        Ok(self)
    }

    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        if !keys.finalize() {
            Err(Traversal::TooLong(0))?;
        }
        Ok(self)
    }
}
