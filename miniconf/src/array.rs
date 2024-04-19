use crate::{Error, Increment, Key, Keys, Metadata, TreeDeserialize, TreeKey, TreeSerialize};
use serde::{de::Deserialize, Deserializer, Serialize, Serializer};

/// Returns the number of digits required to format an integer less than `x`.
const fn digits(x: usize) -> usize {
    let mut max = 10;
    let mut digits = 1;

    while x > max {
        max *= 10;
        digits += 1;
    }
    digits
}

// Y >= 2
macro_rules! depth {
    ($($y:literal)+) => {$(
        impl<T: TreeKey<{$y - 1}>, const N: usize> TreeKey<$y> for [T; N] {
            fn name_to_index(value: &str) -> Option<usize> {
                value.parse().ok()
            }

            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, &str, usize) -> Result<(), E>,
            {
                let index = keys.lookup::<$y, Self, E>(N)?;
                if index >= N {
                    return Err(Error::NotFound(1));
                }
                func(index, itoa::Buffer::new().format(index), N)?;
                T::traverse_by_key(keys, func).increment()
            }

            fn metadata() -> Metadata {
                let mut meta = T::metadata();

                meta.max_length += digits(N);
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
                let index = keys.lookup::<$y, Self, S::Error>(N)?;
                let item = self.get(index).ok_or(Error::NotFound(1))?;
                item.serialize_by_key(keys, ser).increment()
            }
        }

        impl<'de, T: TreeDeserialize<'de, {$y - 1}>, const N: usize> TreeDeserialize<'de, $y> for [T; N] {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                let index = keys.lookup::<$y, Self, D::Error>(N)?;
                let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
                item.deserialize_by_key(keys, de).increment()
            }
        }
    )+}
}
depth!(2 3 4 5 6 7 8);

// Y == 1
impl<T, const N: usize> TreeKey for [T; N] {
    fn name_to_index(value: &str) -> Option<usize> {
        value.parse().ok()
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, &str, usize) -> Result<(), E>,
    {
        let key = keys.next(N).ok_or(Error::TooShort(0))?;
        match key.find::<1, Self>() {
            Some(index) if index < N => {
                func(index, itoa::Buffer::new().format(index), N)?;
                Ok(1)
            }
            _ => Err(Error::NotFound(1)),
        }
    }

    fn metadata() -> Metadata {
        Metadata {
            max_length: digits(N),
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
        let index = keys.lookup::<1, Self, S::Error>(N)?;
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
        let index = keys.lookup::<1, Self, D::Error>(N)?;
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
