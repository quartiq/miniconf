use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{Error, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk};

impl<T: TreeKey> TreeKey for Option<T> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: TreeSerialize> TreeSerialize for Option<T> {
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        self.as_ref()
            .ok_or(Traversal::Absent(0))?
            .serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Option<T> {
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        self.as_mut()
            .ok_or(Traversal::Absent(0))?
            .deserialize_by_key(keys, de)
    }
}

impl<T: TreeAny> TreeAny for Option<T> {
    fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        self.as_ref()
            .ok_or(Traversal::Absent(0))?
            .ref_any_by_key(keys)
    }

    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        self.as_mut()
            .ok_or(Traversal::Absent(0))?
            .mut_any_by_key(keys)
    }
}
