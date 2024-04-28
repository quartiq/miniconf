use core::any::Any;

use crate::{
    digits, increment_result, Error, Keys, Metadata, TreeAny, TreeDeserialize, TreeKey,
    TreeSerialize,
};
use serde::{de::Deserialize, Deserializer, Serialize, Serializer};

// Y >= 2
macro_rules! depth {
    ($($y:literal)+) => {$(
        impl<T: TreeKey<{$y - 1}>, const N: usize> TreeKey<$y> for [T; N] {
            fn len() -> usize {
                N
            }

            fn name_to_index(value: &str) -> Option<usize> {
                value.parse().ok()
            }

            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                let index = keys.next::<$y, Self>()?;
                if index >= N {
                    return Err(Error::NotFound(1));
                }
                func(index, None, N)?;
                increment_result(T::traverse_by_key(keys, func))
            }

            fn metadata() -> Metadata {
                let mut meta = T::metadata();

                meta.max_length += digits::<10>(N);
                meta.max_depth += 1;
                meta.count *= N;

                meta
            }
        }

        impl<T: TreeSerialize<{$y - 1}>, const N: usize> TreeSerialize<$y> for [T; N] {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                let index = keys.next::<$y, Self>()?;
                let item = self.get(index).ok_or(Error::NotFound(1))?;
                increment_result(item.serialize_by_key(keys, ser))
            }
        }

        impl<'de, T: TreeDeserialize<'de, {$y - 1}>, const N: usize> TreeDeserialize<'de, $y> for [T; N] {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                let index = keys.next::<$y, Self>()?;
                let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
                increment_result(item.deserialize_by_key(keys, de))
            }
        }

        impl<T: TreeAny<{$y - 1}>, const N: usize> TreeAny<$y> for [T; N] {
            fn get_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Error<()>>
            where
                K: Keys,
            {
                let index = keys.next::<1, Self>()?;
                let item = self.get(index).ok_or(Error::NotFound(1))?;
                item.get_by_key(keys).map_err(Error::increment)
            }

            fn get_mut_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Error<()>>
            where
                K: Keys,
            {
                let index = keys.next::<1, Self>()?;
                let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
                item.get_mut_by_key(keys).map_err(Error::increment)
            }
        }
    )+}
}
depth!(2 3 4 5 6 7 8);

// Y == 1
impl<T, const N: usize> TreeKey for [T; N] {
    fn len() -> usize {
        N
    }

    fn name_to_index(value: &str) -> Option<usize> {
        value.parse().ok()
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        let index = keys.next::<1, Self>()?;
        if index < N {
            func(index, None, N)?;
            Ok(1)
        } else {
            Err(Error::NotFound(1))
        }
    }

    fn metadata() -> Metadata {
        Metadata {
            max_length: digits::<10>(N),
            max_depth: 1,
            count: N,
        }
    }
}

impl<T: Serialize, const N: usize> TreeSerialize for [T; N] {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        let index = keys.next::<1, Self>()?;
        let item = self.get(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if !keys.is_empty() {
            Err(Error::TooLong(1))
        } else {
            item.serialize(ser)?;
            Ok(1)
        }
    }
}

impl<'de, T: Deserialize<'de>, const N: usize> TreeDeserialize<'de> for [T; N] {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        let index = keys.next::<1, Self>()?;
        let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if !keys.is_empty() {
            Err(Error::TooLong(1))
        } else {
            *item = T::deserialize(de)?;
            Ok(1)
        }
    }
}

impl<T: Any, const N: usize> TreeAny for [T; N] {
    fn get_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Error<()>>
    where
        K: Keys,
    {
        let index = keys.next::<1, Self>()?;
        let item = self.get(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if !keys.is_empty() {
            Err(Error::TooLong(1))
        } else {
            Ok(item)
        }
    }

    fn get_mut_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Error<()>>
    where
        K: Keys,
    {
        let index = keys.next::<1, Self>()?;
        let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if !keys.is_empty() {
            Err(Error::TooLong(1))
        } else {
            Ok(item)
        }
    }
}
