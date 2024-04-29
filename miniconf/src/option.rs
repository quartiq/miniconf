use core::any::Any;

use crate::{Error, Keys, Metadata, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize};
use serde::{de::Deserialize, Deserializer, Serialize, Serializer};

// `Option` does not add to the path hierarchy (does not consume from `keys` or call `func`).
// But it does add one Tree API layer between its `Tree<Y>` level
// and its inner type `Tree<Y'>` level: `Y' = Y - 1`.

// the Y >= 2 cases:
macro_rules! depth {
    ($($y:literal)+) => {$(
        impl<T: TreeKey<{$y - 1}>> TreeKey<$y> for Option<T> {
            fn len() -> usize {
                0
            }

            fn name_to_index(_value: &str) -> Option<usize> {
                None
            }

            fn metadata() -> Metadata {
                T::metadata()
            }

            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize<{$y - 1}>> TreeSerialize<$y> for Option<T> {
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                self
                    .as_ref()
                    .ok_or(Traversal::Absent(0).into())
                    .and_then(|inner| inner.serialize_by_key(keys, ser))
            }
        }

        impl<'de, T: TreeDeserialize<'de, {$y - 1}>> TreeDeserialize<'de, $y> for Option<T> {
            fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self
                    .as_mut()
                    .ok_or(Traversal::Absent(0).into())
                    .and_then(|inner| inner.deserialize_by_key(keys, de))
            }
        }

        impl<T: TreeAny<{$y - 1}>> TreeAny<$y> for Option<T> {
            fn get_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
            where
                K: Keys,
            {
                self.as_ref().ok_or(Traversal::Absent(0)).and_then(|inner| inner.get_by_key(keys))
            }

            fn get_mut_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
            where
                K: Keys,
            {
                self.as_mut().ok_or(Traversal::Absent(0)).and_then(|inner| inner.get_mut_by_key(keys))
            }
        }
    )+}
}
depth!(2 3 4 5 6 7 8 9 10 11 12 13 14 15 16);

// Y == 1
impl<T> TreeKey for Option<T> {
    fn len() -> usize {
        0
    }

    fn name_to_index(_value: &str) -> Option<usize> {
        None
    }

    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }

    fn traverse_by_key<K, F, E>(_keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        Ok(0)
    }
}

impl<T: Serialize> TreeSerialize for Option<T> {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        keys.finalize::<0>()?;
        if let Some(inner) = self {
            inner
                .serialize(ser)
                .map_err(|err| Error::Inner(0, err))
                .and(Ok(0))
        } else {
            Err(Traversal::Absent(0).into())
        }
    }
}

impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Option<T> {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize::<0>()?;
        if let Some(inner) = self {
            *inner = T::deserialize(de).map_err(|err| Error::Inner(0, err))?;
            Ok(0)
        } else {
            Err(Traversal::Absent(0).into())
        }
    }
}

impl<T: Any> TreeAny for Option<T> {
    fn get_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize::<0>()?;
        if let Some(inner) = self {
            Ok(inner)
        } else {
            Err(Traversal::Absent(0))
        }
    }

    fn get_mut_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize::<0>()?;
        if let Some(inner) = self {
            Ok(inner)
        } else {
            Err(Traversal::Absent(0))
        }
    }
}
