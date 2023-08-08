use crate::{Error, Key, Metadata, TreeDeserialize, TreeKey, TreeSerialize};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};

macro_rules! depth {
    ($($d:literal)+) => {$(
        impl<T: TreeKey<{$d - 1}>> TreeKey<$d> for Option<T> {
            fn name_to_index(_value: &str) -> Option<usize> {
                None
            }

            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
            where
                K: Iterator,
                K::Item: Key,
                F: FnMut(usize, &str) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }

            fn metadata() -> Metadata {
                T::metadata()
            }
        }

        impl<T: TreeSerialize<{$d - 1}>> TreeSerialize<$d> for Option<T> {
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Iterator,
                K::Item: Key,
                S: Serializer,
            {
                if let Some(inner) = self {
                    inner.serialize_by_key(keys, ser)
                } else {
                    Err(Error::Absent(0))
                }
            }
        }

        impl<T: TreeDeserialize<{$d - 1}>> TreeDeserialize<$d> for Option<T> {
            fn deserialize_by_key<'de, K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Iterator,
                K::Item: Key,
                D: Deserializer<'de>,
            {
                if let Some(inner) = self {
                    inner.deserialize_by_key(keys, de)
                } else {
                    Err(Error::Absent(0))
                }
            }
        }
    )+}
}

depth!(2 3 4 5 6 7 8);

impl<T> TreeKey for Option<T> {
    fn name_to_index(_value: &str) -> Option<usize> {
        None
    }

    fn traverse_by_key<K, F, E>(_keys: K, _func: F) -> Result<usize, Error<E>>
    where
        F: FnMut(usize, &str) -> Result<(), E>,
    {
        Ok(0)
    }

    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }
}

impl<T: Serialize> TreeSerialize for Option<T> {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Iterator,
        S: Serializer,
    {
        if keys.next().is_some() {
            Err(Error::TooLong(0))
        } else if let Some(inner) = self {
            Serialize::serialize(inner, ser)?;
            Ok(0)
        } else {
            Err(Error::Absent(0))
        }
    }
}

impl<T: DeserializeOwned> TreeDeserialize for Option<T> {
    fn deserialize_by_key<'de, K, D>(
        &mut self,
        mut keys: K,
        de: D,
    ) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        D: Deserializer<'de>,
    {
        if keys.next().is_some() {
            Err(Error::TooLong(0))
        } else if let Some(inner) = self {
            *inner = Deserialize::deserialize(de)?;
            Ok(0)
        } else {
            Err(Error::Absent(0))
        }
    }
}
