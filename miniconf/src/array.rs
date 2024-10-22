use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{
    Error, KeyLookup, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

impl<T: TreeKey, const N: usize> TreeKey for [T; N] {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        W::internal().merge(&T::traverse_all::<W>()?, None, &KeyLookup::homogeneous(N))
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        let index = keys.next(&KeyLookup::homogeneous(N))?;
        func(index, None, N).map_err(|err| Error::Inner(1, err))?;
        Error::increment_result(T::traverse_by_key(keys, func))
    }
}

impl<T: TreeSerialize, const N: usize> TreeSerialize for [T; N] {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        let index = keys.next(&KeyLookup::homogeneous(N))?;
        Error::increment_result(self[index].serialize_by_key(keys, ser))
    }
}

impl<'de, T: TreeDeserialize<'de>, const N: usize> TreeDeserialize<'de> for [T; N] {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        let index = keys.next(&KeyLookup::homogeneous(N))?;
        Error::increment_result(self[index].deserialize_by_key(keys, de))
    }
}

impl<T: TreeAny, const N: usize> TreeAny for [T; N] {
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        let index = keys.next(&KeyLookup::homogeneous(N))?;
        self[index]
            .ref_any_by_key(keys)
            .map_err(Traversal::increment)
    }

    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        let index = keys.next(&KeyLookup::homogeneous(N))?;
        self[index]
            .mut_any_by_key(keys)
            .map_err(Traversal::increment)
    }
}
