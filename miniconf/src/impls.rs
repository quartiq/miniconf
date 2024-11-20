use core::cell::{Cell, RefCell};
use core::ops::{Bound, Range, RangeFrom, RangeInclusive, RangeTo};
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
                W::internal(&[$(&$t::traverse_all()?, )+], &KeyLookup::numbered($n))
            }

            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                let k = KeyLookup::numbered($n);
                let index = keys.next(&k)?;
                func(index, None, k.len()).map_err(|err| Error::Inner(1, err))?;
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
                let index = keys.next(&KeyLookup::numbered($n))?;
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
                let index = keys.next(&KeyLookup::numbered($n))?;
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
                let index = keys.next(&KeyLookup::numbered($n))?;
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
                let index = keys.next(&KeyLookup::numbered($n))?;
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
        W::internal(&[&T::traverse_all()?], &KeyLookup::homogeneous(N))
    }

    fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        let () = Assert::<N, 0>::GREATER; // internal nodes must have at least one leaf
        let k = KeyLookup::homogeneous(N);
        let index = keys.next(&k)?;
        func(index, None, k.len()).map_err(|err| Error::Inner(1, err))?;
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
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    #[inline]
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: TreeSerialize> TreeSerialize for Option<T> {
    #[inline]
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
    #[inline]
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
    #[inline]
    fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        self.as_ref()
            .ok_or(Traversal::Absent(0))?
            .ref_any_by_key(keys)
    }

    #[inline]
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

const RESULT_LOOKUP: KeyLookup = KeyLookup::Named(&["Ok", "Err"]);

impl<T: TreeKey, E: TreeKey> TreeKey for Result<T, E> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        W::internal(&[&T::traverse_all()?, &E::traverse_all()?], &RESULT_LOOKUP)
    }

    #[inline]
    fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, Error<G>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
    {
        Error::increment_result(match keys.next(&RESULT_LOOKUP)? {
            0 => T::traverse_by_key(keys, func),
            1 => E::traverse_by_key(keys, func),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeSerialize, E: TreeSerialize> TreeSerialize for Result<T, E> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        Error::increment_result(match (keys.next(&RESULT_LOOKUP)?, self) {
            (0, Ok(value)) => value.serialize_by_key(keys, ser),
            (1, Err(value)) => value.serialize_by_key(keys, ser),
            _ => Err(Traversal::Absent(0).into()),
        })
    }
}

impl<'de, T: TreeDeserialize<'de>, E: TreeDeserialize<'de>> TreeDeserialize<'de> for Result<T, E> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        Error::increment_result(match (keys.next(&RESULT_LOOKUP)?, self) {
            (0, Ok(value)) => value.deserialize_by_key(keys, de),
            (1, Err(value)) => value.deserialize_by_key(keys, de),
            _ => Err(Traversal::Absent(0).into()),
        })
    }
}

impl<T: TreeAny, E: TreeAny> TreeAny for Result<T, E> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        match (keys.next(&RESULT_LOOKUP)?, self) {
            (0, Ok(value)) => value.ref_any_by_key(keys),
            (1, Err(value)) => value.ref_any_by_key(keys),
            _ => Err(Traversal::Absent(0)),
        }
        .map_err(Traversal::increment)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        match (keys.next(&RESULT_LOOKUP)?, self) {
            (0, Ok(value)) => value.mut_any_by_key(keys),
            (1, Err(value)) => value.mut_any_by_key(keys),
            _ => Err(Traversal::Absent(0)),
        }
        .map_err(Traversal::increment)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

const BOUND_LOOKUP: KeyLookup = KeyLookup::Named(&["Included", "Excluded"]);

impl<T: TreeKey> TreeKey for Bound<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        let t: W = T::traverse_all()?;
        W::internal(&[&t; 2], &BOUND_LOOKUP)
    }

    #[inline]
    fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, Error<G>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
    {
        Error::increment_result(match keys.next(&BOUND_LOOKUP)? {
            0..=1 => T::traverse_by_key(keys, func),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeSerialize> TreeSerialize for Bound<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        Error::increment_result(match (keys.next(&BOUND_LOOKUP)?, self) {
            (0, Self::Included(value)) | (1, Self::Excluded(value)) => {
                value.serialize_by_key(keys, ser)
            }
            _ => Err(Traversal::Absent(0).into()),
        })
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Bound<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        Error::increment_result(match (keys.next(&BOUND_LOOKUP)?, self) {
            (0, Self::Included(value)) | (1, Self::Excluded(value)) => {
                value.deserialize_by_key(keys, de)
            }
            _ => Err(Traversal::Absent(0).into()),
        })
    }
}

impl<T: TreeAny> TreeAny for Bound<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        match (keys.next(&BOUND_LOOKUP)?, self) {
            (0, Self::Included(value)) | (1, Self::Excluded(value)) => value.ref_any_by_key(keys),
            _ => Err(Traversal::Absent(0)),
        }
        .map_err(Traversal::increment)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        match (keys.next(&BOUND_LOOKUP)?, self) {
            (0, Self::Included(value)) | (1, Self::Excluded(value)) => value.mut_any_by_key(keys),
            _ => Err(Traversal::Absent(0)),
        }
        .map_err(Traversal::increment)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

const RANGE_LOOKUP: KeyLookup = KeyLookup::Named(&["start", "end"]);

impl<T: TreeKey> TreeKey for Range<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        let t: W = T::traverse_all()?;
        W::internal(&[&t; 2], &RANGE_LOOKUP)
    }

    #[inline]
    fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, Error<G>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
    {
        Error::increment_result(match keys.next(&RANGE_LOOKUP)? {
            0..=1 => T::traverse_by_key(keys, func),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeSerialize> TreeSerialize for Range<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        Error::increment_result(match keys.next(&RANGE_LOOKUP)? {
            0 => self.start.serialize_by_key(keys, ser),
            1 => self.end.serialize_by_key(keys, ser),
            _ => unreachable!(),
        })
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Range<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        Error::increment_result(match keys.next(&RANGE_LOOKUP)? {
            0 => self.start.deserialize_by_key(keys, de),
            1 => self.end.deserialize_by_key(keys, de),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeAny> TreeAny for Range<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        match keys.next(&RANGE_LOOKUP)? {
            0 => self.start.ref_any_by_key(keys),
            1 => self.end.ref_any_by_key(keys),
            _ => unreachable!(),
        }
        .map_err(Traversal::increment)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        match keys.next(&RANGE_LOOKUP)? {
            0 => self.start.mut_any_by_key(keys),
            1 => self.end.mut_any_by_key(keys),
            _ => unreachable!(),
        }
        .map_err(Traversal::increment)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeKey> TreeKey for RangeInclusive<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        let t: W = T::traverse_all()?;
        W::internal(&[&t; 2], &RANGE_LOOKUP)
    }

    #[inline]
    fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, Error<G>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
    {
        Error::increment_result(match keys.next(&RANGE_LOOKUP)? {
            0..=1 => T::traverse_by_key(keys, func),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeSerialize> TreeSerialize for RangeInclusive<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        Error::increment_result(match keys.next(&RANGE_LOOKUP)? {
            0 => self.start().serialize_by_key(keys, ser),
            1 => self.end().serialize_by_key(keys, ser),
            _ => unreachable!(),
        })
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

const RANGE_FROM_LOOKUP: KeyLookup = KeyLookup::Named(&["start"]);

impl<T: TreeKey> TreeKey for RangeFrom<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        W::internal(&[&T::traverse_all()?], &RANGE_FROM_LOOKUP)
    }

    #[inline]
    fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, Error<G>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
    {
        Error::increment_result(match keys.next(&RANGE_FROM_LOOKUP)? {
            0 => T::traverse_by_key(keys, func),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeSerialize> TreeSerialize for RangeFrom<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        Error::increment_result(match keys.next(&RANGE_FROM_LOOKUP)? {
            0 => self.start.serialize_by_key(keys, ser),
            _ => unreachable!(),
        })
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RangeFrom<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        Error::increment_result(match keys.next(&RANGE_FROM_LOOKUP)? {
            0 => self.start.deserialize_by_key(keys, de),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeAny> TreeAny for RangeFrom<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        match keys.next(&RANGE_FROM_LOOKUP)? {
            0 => self.start.ref_any_by_key(keys),
            _ => unreachable!(),
        }
        .map_err(Traversal::increment)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        match keys.next(&RANGE_FROM_LOOKUP)? {
            0 => self.start.mut_any_by_key(keys),
            _ => unreachable!(),
        }
        .map_err(Traversal::increment)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

const RANGE_TO_LOOKUP: KeyLookup = KeyLookup::Named(&["end"]);

impl<T: TreeKey> TreeKey for RangeTo<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        W::internal(&[&T::traverse_all()?], &RANGE_TO_LOOKUP)
    }

    #[inline]
    fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, Error<G>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
    {
        Error::increment_result(match keys.next(&RANGE_TO_LOOKUP)? {
            0 => T::traverse_by_key(keys, func),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeSerialize> TreeSerialize for RangeTo<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        Error::increment_result(match keys.next(&RANGE_TO_LOOKUP)? {
            0 => self.end.serialize_by_key(keys, ser),
            _ => unreachable!(),
        })
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RangeTo<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        Error::increment_result(match keys.next(&RANGE_TO_LOOKUP)? {
            0 => self.end.deserialize_by_key(keys, de),
            _ => unreachable!(),
        })
    }
}

impl<T: TreeAny> TreeAny for RangeTo<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        match keys.next(&RANGE_TO_LOOKUP)? {
            0 => self.end.ref_any_by_key(keys),
            _ => unreachable!(),
        }
        .map_err(Traversal::increment)
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        match keys.next(&RANGE_TO_LOOKUP)? {
            0 => self.end.mut_any_by_key(keys),
            _ => unreachable!(),
        }
        .map_err(Traversal::increment)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeKey> TreeKey for Cell<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    #[inline]
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: TreeSerialize + Copy> TreeSerialize for Cell<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        self.get().serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Cell<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        self.get_mut().deserialize_by_key(keys, de)
    }
}

impl<T: TreeAny> TreeAny for Cell<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        Err(Traversal::Access(0, "Can't leak out of Cell"))
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        self.get_mut().mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeKey> TreeKey for RefCell<T> {
    #[inline]
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        T::traverse_all()
    }

    #[inline]
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: TreeSerialize> TreeSerialize for RefCell<T> {
    #[inline]
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        self.try_borrow()
            .or(Err(Traversal::Access(0, "Borrowed")))?
            .serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RefCell<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        self.get_mut().deserialize_by_key(keys, de)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &RefCell<T> {
    #[inline]
    fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        self.try_borrow_mut()
            .or(Err(Traversal::Access(0, "Borrowed")))?
            .deserialize_by_key(keys, de)
    }
}

impl<T: TreeAny> TreeAny for RefCell<T> {
    #[inline]
    fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        Err(Traversal::Access(0, "Can't leak out of RefCell"))
    }

    #[inline]
    fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        self.get_mut().mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

#[cfg(feature = "alloc")]
mod _alloc {
    use super::*;
    extern crate alloc;
    use alloc::{borrow::Cow, boxed::Box, rc, rc::Rc, sync, sync::Arc};

    impl<T: TreeKey> TreeKey for Box<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for Box<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Box<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            (**self).deserialize_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for Box<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
        where
            K: Keys,
        {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
        where
            K: Keys,
        {
            (**self).mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<'a, T: TreeKey + Clone> TreeKey for Cow<'a, T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<'a, T: TreeSerialize + Clone> TreeSerialize for Cow<'a, T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'a, 'de, T: TreeDeserialize<'de> + Clone> TreeDeserialize<'de> for Cow<'a, T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.to_mut().deserialize_by_key(keys, de)
        }
    }

    impl<'a, T: TreeAny + Clone> TreeAny for Cow<'a, T> {
        #[inline]
        fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
        where
            K: Keys,
        {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
        where
            K: Keys,
        {
            self.to_mut().mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for Rc<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for Rc<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Rc<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            Rc::get_mut(self)
                .ok_or(Traversal::Access(0, "Reference is taken"))?
                .deserialize_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for Rc<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
        where
            K: Keys,
        {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
        where
            K: Keys,
        {
            Rc::get_mut(self)
                .ok_or(Traversal::Access(0, "Reference is taken"))?
                .mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for rc::Weak<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for rc::Weak<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            self.upgrade()
                .ok_or(Traversal::Absent(0))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for rc::Weak<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.upgrade()
                .ok_or(Traversal::Absent(0))?
                .deserialize_by_key(keys, de)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for Arc<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for Arc<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Arc<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            Arc::get_mut(self)
                .ok_or(Traversal::Access(0, "Reference is taken"))?
                .deserialize_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for Arc<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
        where
            K: Keys,
        {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
        where
            K: Keys,
        {
            Arc::get_mut(self)
                .ok_or(Traversal::Access(0, "Reference is taken"))?
                .mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for sync::Weak<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for sync::Weak<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            self.upgrade()
                .ok_or(Traversal::Absent(0))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for sync::Weak<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.upgrade()
                .ok_or(Traversal::Absent(0))?
                .deserialize_by_key(keys, de)
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

#[cfg(feature = "std")]
mod _std {
    use super::*;
    use std::sync::{Mutex, RwLock};

    impl<T: TreeKey> TreeKey for Mutex<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for Mutex<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            self.lock()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Mutex<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.get_mut()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .deserialize_by_key(keys, de)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &Mutex<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            (*self)
                .lock()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .deserialize_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for Mutex<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, Traversal>
        where
            K: Keys,
        {
            Err(Traversal::Access(0, "Can't leak out of Mutex"))
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
        where
            K: Keys,
        {
            self.get_mut()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for RwLock<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> Result<W, W::Error> {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for RwLock<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            self.read()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &RwLock<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.write()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .deserialize_by_key(keys, de)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RwLock<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.get_mut()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .deserialize_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for RwLock<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, Traversal>
        where
            K: Keys,
        {
            Err(Traversal::Access(0, "Can't leak out of RwLock"))
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
        where
            K: Keys,
        {
            self.get_mut()
                .or(Err(Traversal::Access(0, "Poisoned")))?
                .mut_any_by_key(keys)
        }
    }
}
