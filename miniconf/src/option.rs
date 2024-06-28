use core::any::Any;

use crate::{Error, Keys, Metadata, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize};
use serde::{de::Deserialize, Deserializer, Serialize, Serializer};

// `Option` does not add to the path hierarchy (does not consume from `keys` or call `func`).
// But it does add one Tree API layer between its `Tree<Y>` level
// and its inner type `Tree<Y'>` level: `Y' = Y - 1`.
// Otherwise we would not be able to distinguish between an augmented `Option<T>: TreeKey<0>`
// and a plain-serde `Option<T>: Serialize/Deserialize/Any` in situations where the trait to
// use is implicit (e.g. in an array). Also the bounds heuristics in the
// derive macros assume that a field type `#[tree(depth=Y)] F<T>` calls its generic types at
// `TreeKey<{Y - 1}>`. The latter could be ameliorated with a `bounds` derive macro attribute.

fn get<'a, T, K: Keys>(opt: &'a Option<T>, keys: &mut K, drain: bool) -> Result<&'a T, Traversal> {
    if drain && !keys.is_empty() {
        Err(Traversal::TooLong(0))
    } else {
        opt.as_ref().ok_or(Traversal::Absent(0))
    }
}

fn get_mut<'a, T, K: Keys>(
    opt: &'a mut Option<T>,
    keys: &mut K,
    drain: bool,
) -> Result<&'a mut T, Traversal> {
    if drain && !keys.is_empty() {
        Err(Traversal::TooLong(0))
    } else {
        opt.as_mut().ok_or(Traversal::Absent(0))
    }
}

// the Y >= 2 cases:
macro_rules! depth {
    ($($y:literal)+) => {$(
        impl<T: TreeKey<{$y - 1}>> TreeKey<$y> for Option<T> {
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
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                let inner = get(self, &mut keys, false)?;
                inner.serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de, {$y - 1}>> TreeDeserialize<'de, $y> for Option<T> {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                let inner = get_mut(self, &mut keys, false)?;
                inner.deserialize_by_key(keys, de)
            }
        }

        impl<T: TreeAny<{$y - 1}>> TreeAny<$y> for Option<T> {
            fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
            where
                K: Keys,
            {
                let inner = get(self, &mut keys, false)?;
                inner.ref_any_by_key(keys)
            }

            fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
            where
                K: Keys,
            {
                let inner = get_mut(self, &mut keys, false)?;
                inner.mut_any_by_key(keys)
            }
        }
    )+}
}
depth!(2 3 4 5 6 7 8 9 10 11 12 13 14 15 16);

// Y == 1
impl<T> TreeKey for Option<T> {
    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }

    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        if !keys.is_empty() {
            Err(Traversal::TooLong(0).into())
        } else {
            Ok(0)
        }
    }
}

impl<T: Serialize> TreeSerialize for Option<T> {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        let inner = get(self, &mut keys, true)?;
        inner.serialize(ser).map_err(|err| Error::Inner(0, err))?;
        Ok(0)
    }
}

impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Option<T> {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        let inner = get_mut(self, &mut keys, true)?;
        *inner = T::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        Ok(0)
    }
}

impl<T: Any> TreeAny for Option<T> {
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        let inner = get(self, &mut keys, true)?;
        Ok(inner)
    }

    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        let inner = get_mut(self, &mut keys, true)?;
        Ok(inner)
    }
}
