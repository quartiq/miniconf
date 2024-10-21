use core::{
    any::Any,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk};

/// Transparent tree level Y break
#[derive(Clone, Copy, Default, Debug, PartialEq, PartialOrd, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Pad<T: ?Sized, const X: usize>(pub T);

impl<T: ?Sized, const X: usize> Deref for Pad<T, X> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized, const X: usize> DerefMut for Pad<T, X> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T, const X: usize> Pad<T, X> {
    /// Extract just the inner
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<const Y: usize, const X: usize, T: TreeKey<X>> TreeKey<Y> for Pad<T, X> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all::<W>()
    }

    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<const Y: usize, const X: usize, T: TreeSerialize<X>> TreeSerialize<Y> for Pad<T, X> {
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        T::serialize_by_key(&self.0, keys, ser)
    }
}

impl<'de, const Y: usize, const X: usize, T: TreeDeserialize<'de, X>> TreeDeserialize<'de, Y>
    for Pad<T, X>
{
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        T::deserialize_by_key(&mut self.0, keys, de)
    }
}

impl<const Y: usize, const X: usize, T: TreeAny<X>> TreeAny<Y> for Pad<T, X> {
    fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        T::ref_any_by_key(&self.0, keys)
    }

    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        T::mut_any_by_key(&mut self.0, keys)
    }
}
