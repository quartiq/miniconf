use core::{any::Any, num::NonZero};

use serde::{Deserializer, Serializer};

use crate::{
    Error, KeyLookup, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

/////////////////////////////////////////////////////////////////////////////////////////

macro_rules! impl_tuple {
    ($n:literal $($i:tt $t:ident)+) => {
        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeKey),+> TreeKey for ($($t,)+) {
            fn traverse_all<W: Walk>() -> Result<W, W::Error> {
                let k = KeyLookup::homogeneous($n);
                let mut walk = W::internal();
                $(walk = walk.merge(&$t::traverse_all()?, Some($i), &k)?;)+
                Ok(walk)
            }

            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                let k = KeyLookup::homogeneous($n);
                let index = keys.next(&k)?;
                func(index, None, k.len).map_err(|err| Error::Inner(1, err))?;
                Error::increment_result(match index {
                    $($i => $t::traverse_by_key(keys, func),)+
                    _ => unreachable!()
                })
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeSerialize),+> TreeSerialize for ($($t,)+) {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                let index = keys.next(&KeyLookup::homogeneous($n))?;
                Error::increment_result(match index {
                    $($i => self.$i.serialize_by_key(keys, ser),)+
                    _ => unreachable!()
                })
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<'de, $($t: TreeDeserialize<'de>),+> TreeDeserialize<'de> for ($($t,)+) {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                let index = keys.next(&KeyLookup::homogeneous($n))?;
                Error::increment_result(match index {
                    $($i => self.$i.deserialize_by_key(keys, de),)+
                    _ => unreachable!()
                })
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeAny),+> TreeAny for ($($t,)+) {
            fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
            where
                K: Keys,
            {
                let index = keys.next(&KeyLookup::homogeneous($n))?;
                let ret: Result<_, _> = match index {
                    $($i => self.$i.ref_any_by_key(keys),)+
                    _ => unreachable!()
                };
                ret.map_err(Traversal::increment)
            }

            fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
            where
                K: Keys,
            {
                let index = keys.next(&KeyLookup::homogeneous($n))?;
                let ret: Result<_, _> = match index {
                    $($i => self.$i.mut_any_by_key(keys),)+
                    _ => unreachable!()
                };
                ret.map_err(Traversal::increment)
            }
        }
    }
}
// Note: internal nodes must have at least one leaf
impl_tuple!(1 0 T0);
impl_tuple!(2 0 T0 1 T1);
impl_tuple!(3 0 T0 1 T1 2 T2);
impl_tuple!(4 0 T0 1 T1 2 T2 3 T3);
impl_tuple!(5 0 T0 1 T1 2 T2 3 T3 4 T4);
impl_tuple!(6 0 T0 1 T1 2 T2 3 T3 4 T4 5 T5);
impl_tuple!(7 0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6);
impl_tuple!(8 0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7);

/////////////////////////////////////////////////////////////////////////////////////////

struct Assert<const L: usize, const R: usize>;
impl<const L: usize, const R: usize> Assert<L, R> {
    const GREATER: () = assert!(L > R);
}

impl<T: TreeKey, const N: usize> TreeKey for [T; N] {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        let () = Assert::<N, 0>::GREATER; // internal nodes must have at least one leaf
        W::internal().merge(&T::traverse_all()?, None, &KeyLookup::homogeneous(N))
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        let () = Assert::<N, 0>::GREATER; // internal nodes must have at least one leaf
        let k = KeyLookup::homogeneous(N);
        let index = keys.next(&k)?;
        func(index, None, k.len).map_err(|err| Error::Inner(1, err))?;
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

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeKey> TreeKey for Option<T> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
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

/////////////////////////////////////////////////////////////////////////////////////////
