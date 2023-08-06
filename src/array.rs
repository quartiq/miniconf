use crate::{Error, Increment, Key, Metadata, TreeDeserialize, TreeKey, TreeSerialize};

/// An array that exposes each element through [`Miniconf`].
///
/// # Design
///
/// With `#[miniconf(defer(D))]` and a depth `D > 1` for an
/// [`[T; N]`](array), each item of the array is accessed as a [`Miniconf`] tree.
/// For a depth `D = 0`, the entire array is accessed as one atomic
/// value. For `D = 1` each index of the array is is instead accessed as
/// one atomic value.
///
/// The type to use depends on what data is contained in your array. If your array contains
/// `Miniconf` items, you can (and often want to) use `D >= 2`.
/// However, if each element in your list is individually configurable as a single value (e.g. a list
/// of `u32`), then you must use `D = 1` or `D = 0` if all items are to be accessed simultaneously.
/// For e.g. `[[T; 2]; 3] where T: Miniconf<3>` you may want to use `D = 5` (note that `D <= 2`
/// will also work if `T: Serialize + DeserializeOwned`).

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
                S: serde::Serializer,
            {
                let key = keys.next().ok_or(Error::TooShort(0))?;
                let index = key.find::<$d, Self>().ok_or(Error::NotFound(1))?;
                let item = self.get(index).ok_or(Error::NotFound(1))?;
                item.serialize_by_key(keys, ser).increment()
            }
        }

        impl<T: TreeDeserialize<{$d - 1}>, const N: usize> TreeDeserialize<$d> for [T; N] {
            fn deserialize_by_key<'a, K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Iterator,
                K::Item: Key,
                D: serde::Deserializer<'a>,
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
        let index = match key.find::<1, Self>() {
            Some(i) if i < N => i,
            _ => return Err(Error::NotFound(1)),
        };
        func(index, itoa::Buffer::new().format(index))?;
        Ok(1)
    }

    fn metadata() -> Metadata {
        Metadata {
            max_length: digits(N),
            max_depth: 1,
            count: N,
        }
    }
}

impl<T: serde::Serialize, const N: usize> TreeSerialize for [T; N] {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Iterator,
        K::Item: Key,
        S: serde::Serializer,
    {
        let key = keys.next().ok_or(Error::TooShort(0))?;
        let index = key.find::<1, Self>().ok_or(Error::NotFound(1))?;
        let item = self.get(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if keys.next().is_some() {
            return Err(Error::TooLong(1));
        }
        serde::Serialize::serialize(item, ser)?;
        Ok(1)
    }
}

impl<T: serde::de::DeserializeOwned, const N: usize> TreeDeserialize for [T; N] {
    fn deserialize_by_key<'a, K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        K::Item: Key,
        D: serde::Deserializer<'a>,
    {
        let key = keys.next().ok_or(Error::TooShort(0))?;
        let index: usize = key.find::<1, Self>().ok_or(Error::NotFound(1))?;
        let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
        // Precedence
        if keys.next().is_some() {
            return Err(Error::TooLong(1));
        }
        *item = serde::Deserialize::deserialize(de)?;
        Ok(1)
    }
}
