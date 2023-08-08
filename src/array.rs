use crate::{
    Deserialize, DeserializeOwned, Deserializer, Error, Increment, Key, Metadata, Serialize,
    Serializer, TreeDeserialize, TreeKey, TreeSerialize,
};

/// Returns the number of digits required to format an integer less than `x`.
const fn digits(x: usize) -> usize {
    let mut n = 10;
    let mut num_digits = 1;

    while x > n {
        n *= 10;
        num_digits += 1;
    }
    num_digits
}

macro_rules! depth {
    ($($d:literal)+) => {$(
        impl<T: TreeKey<{$d - 1}>, const N: usize> TreeKey<$d> for [T; N] {
            fn name_to_index(value: &str) -> core::option::Option<usize> {
                value.parse().ok()
            }
            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Iterator,
                K::Item: Key,
                F: FnMut(usize, &str) -> Result<(), E>,
            {
                let key = keys.next().ok_or(Error::TooShort(0))?;
                let index = key.find::<$d, Self>().ok_or(Error::NotFound(1))?;
                if index >= N {
                    return Err(Error::NotFound(1));
                }
                func(index, itoa::Buffer::new().format(index))?;
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

        impl<T: TreeSerialize<{$d - 1}>, const N: usize> TreeSerialize<$d> for [T; N] {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Iterator,
                K::Item: Key,
                S: Serializer,
            {
                let key = keys.next().ok_or(Error::TooShort(0))?;
                let index = key.find::<$d, Self>().ok_or(Error::NotFound(1))?;
                let item = self.get(index).ok_or(Error::NotFound(1))?;
                item.serialize_by_key(keys, ser).increment()
            }
        }

        impl<T: TreeDeserialize<{$d - 1}>, const N: usize> TreeDeserialize<$d> for [T; N] {
            fn deserialize_by_key<'de, K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Iterator,
                K::Item: Key,
                D: Deserializer<'de>,
            {
                let key = keys.next().ok_or(Error::TooShort(0))?;
                let index = key.find::<$d, Self>().ok_or(Error::NotFound(1))?;
                let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
                item.deserialize_by_key(keys, de).increment()
            }
        }
    )+}
}

depth!(2 3 4 5 6 7 8);

impl<T, const N: usize> TreeKey for [T; N] {
    fn name_to_index(value: &str) -> core::option::Option<usize> {
        value.parse().ok()
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Iterator,
        K::Item: Key,
        F: FnMut(usize, &str) -> Result<(), E>,
    {
        let key = keys.next().ok_or(Error::TooShort(0))?;
        match key.find::<1, Self>() {
            Some(index) if index < N => {
                func(index, itoa::Buffer::new().format(index))?;
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
        K: Iterator,
        K::Item: Key,
        S: Serializer,
    {
        let key = keys.next().ok_or(Error::TooShort(0))?;
        let index = key.find::<1, Self>().ok_or(Error::NotFound(1))?;
        let item = self.get(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if keys.next().is_some() {
            Err(Error::TooLong(1))
        } else {
            Serialize::serialize(item, ser)?;
            Ok(1)
        }
    }
}

impl<T: DeserializeOwned, const N: usize> TreeDeserialize for [T; N] {
    fn deserialize_by_key<'de, K, D>(
        &mut self,
        mut keys: K,
        de: D,
    ) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        K::Item: Key,
        D: Deserializer<'de>,
    {
        let key = keys.next().ok_or(Error::TooShort(0))?;
        let index: usize = key.find::<1, Self>().ok_or(Error::NotFound(1))?;
        let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if keys.next().is_some() {
            Err(Error::TooLong(1))
        } else {
            *item = Deserialize::deserialize(de)?;
            Ok(1)
        }
    }
}
