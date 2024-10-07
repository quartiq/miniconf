use core::any::Any;

use serde::{de::Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Error, KeyLookup, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

fn get<'a, const N: usize, K: Keys, T>(
    arr: &'a [T; N],
    keys: &mut K,
    drain: bool,
) -> Result<&'a T, Traversal> {
    let index = keys.next(&KeyLookup {
        len: N,
        names: None,
    })?;
    let item = arr.get(index).ok_or(Traversal::NotFound(1))?;
    if drain && !keys.finalize() {
        Err(Traversal::TooLong(1))
    } else {
        Ok(item)
    }
}

fn get_mut<'a, const N: usize, K: Keys, T>(
    arr: &'a mut [T; N],
    keys: &mut K,
    drain: bool,
) -> Result<&'a mut T, Traversal> {
    let index = keys.next(&KeyLookup {
        len: N,
        names: None,
    })?;
    let item = arr.get_mut(index).ok_or(Traversal::NotFound(1))?;
    if drain && !keys.finalize() {
        Err(Traversal::TooLong(1))
    } else {
        Ok(item)
    }
}

// Y >= 2
macro_rules! depth {
    ($($y:literal)+) => {$(
        impl<T: TreeKey<{$y - 1}>, const N: usize> TreeKey<$y> for [T; N] {
            fn walk<W: Walk>() -> W {
                let mut walk = W::inner();
                walk.merge(None, &T::walk::<W>(), &KeyLookup{len: N, names: None});
                walk
            }

            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                let index = keys.next(&KeyLookup {
                    len: N,
                    names: None,
                })?;
                if index >= N {
                    return Err(Traversal::NotFound(1).into());
                }
                func(index, None, N).map_err(|err| Error::Inner(1, err))?;
                Error::increment_result(T::traverse_by_key(keys, func))
            }
        }

        impl<T: TreeSerialize<{$y - 1}>, const N: usize> TreeSerialize<$y> for [T; N] {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                let item = get(self, &mut keys, false)?;
                Error::increment_result(item.serialize_by_key(keys, ser))
            }
        }

        impl<'de, T: TreeDeserialize<'de, {$y - 1}>, const N: usize> TreeDeserialize<'de, $y> for [T; N] {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                let item = get_mut(self, &mut keys, false)?;
                Error::increment_result(item.deserialize_by_key(keys, de))
            }
        }

        impl<T: TreeAny<{$y - 1}>, const N: usize> TreeAny<$y> for [T; N] {
            fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
            where
                K: Keys,
            {
                let item = get(self, &mut keys, false)?;
                item.ref_any_by_key(keys).map_err(Traversal::increment)
            }

            fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
            where
                K: Keys,
            {
                let item = get_mut(self, &mut keys, false)?;
                item.mut_any_by_key(keys).map_err(Traversal::increment)
            }
        }
    )+}
}
depth!(2 3 4 5 6 7 8 9 10 11 12 13 14 15 16);

// Y == 1
impl<T, const N: usize> TreeKey for [T; N] {
    fn walk<W: Walk>() -> W {
        let mut walk = W::inner();
        walk.merge(
            None,
            &W::leaf(),
            &KeyLookup {
                len: N,
                names: None,
            },
        );
        walk
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        let index = keys.next(&KeyLookup {
            len: N,
            names: None,
        })?;
        if index >= N {
            return Err(Traversal::NotFound(1).into());
        }
        func(index, None, N).map_err(|err| Error::Inner(1, err))?;
        if !keys.finalize() {
            Err(Traversal::TooLong(1).into())
        } else {
            Ok(1)
        }
    }
}

impl<T: Serialize, const N: usize> TreeSerialize for [T; N] {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        let item = get(self, &mut keys, true)?;
        item.serialize(ser).map_err(|err| Error::Inner(1, err))?;
        Ok(1)
    }
}

impl<'de, T: Deserialize<'de>, const N: usize> TreeDeserialize<'de> for [T; N] {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        let item = get_mut(self, &mut keys, true)?;
        *item = T::deserialize(de).map_err(|err| Error::Inner(1, err))?;
        Ok(1)
    }
}

impl<T: Any, const N: usize> TreeAny for [T; N] {
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        Ok(get(self, &mut keys, true)?)
    }

    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        Ok(get_mut(self, &mut keys, true)?)
    }
}
