use core::cell::{Cell, RefCell};
use core::num::NonZeroUsize;
use core::ops::{Bound, Range, RangeFrom, RangeInclusive, RangeTo};
use core::{any::Any, num::NonZero};

use serde::{Deserializer, Serializer};

use crate::{
    Homogeneous, Internal, KeyError, Keys, Numbered, Schema, SerDeError, TreeAny, TreeDeserialize,
    TreeKey, TreeSerialize, ValueError,
};

/////////////////////////////////////////////////////////////////////////////////////////

macro_rules! impl_tuple {
    ($n:literal $($i:tt $t:ident)+) => {
        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeKey),+> TreeKey for ($($t,)+) {
            const SCHEMA: &'static Schema = &Schema {
                meta: None,
                internal: Some(
                    Internal::Numbered(&[$(
                        Numbered{
                            schema: $t::SCHEMA,
                            meta: None
                        },
                    )+])
                )
            };
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeSerialize),+> TreeSerialize for ($($t,)+) {
            fn serialize_by_key<S: Serializer>(
                &self,
                mut keys: impl Keys,
                ser: S
            ) -> Result<S::Ok, SerDeError<S::Error>>
            {
                match Self::SCHEMA.next(&mut keys)? {
                    $($i => self.$i.serialize_by_key(keys, ser),)+
                    _ => unreachable!()
                }
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<'de, $($t: TreeDeserialize<'de>),+> TreeDeserialize<'de> for ($($t,)+) {
            fn deserialize_by_key<D: Deserializer<'de>>(
                &mut self,
                mut keys: impl Keys,
                de: D
            ) -> Result<(), SerDeError<D::Error>>
            {
                match Self::SCHEMA.next(&mut keys)? {
                    $($i => self.$i.deserialize_by_key(keys, de),)+
                    _ => unreachable!()
                }
            }

            fn probe_by_key<D: Deserializer<'de>>(
                mut keys: impl Keys,
                de: D
            ) -> Result<(), SerDeError<D::Error>>
            {
                match Self::SCHEMA.next(&mut keys)? {
                    $($i => $t::probe_by_key(keys, de),)+
                    _ => unreachable!()
                }
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeAny),+> TreeAny for ($($t,)+) {
            fn ref_any_by_key(
                &self,
                mut keys: impl Keys
            ) -> Result<&dyn Any, ValueError>
            {
                match Self::SCHEMA.next(&mut keys)? {
                    $($i => self.$i.ref_any_by_key(keys),)+
                    _ => unreachable!()
                }
            }

            fn mut_any_by_key(
                &mut self,
                mut keys: impl Keys
            ) -> Result<&mut dyn Any, ValueError>
            {
                match Self::SCHEMA.next(&mut keys)? {
                    $($i => self.$i.mut_any_by_key(keys),)+
                    _ => unreachable!()
                }
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

impl<T: TreeKey, const N: usize> TreeKey for [T; N] {
    const SCHEMA: &'static Schema = &Schema {
        meta: None,
        internal: Some(Internal::Homogeneous(Homogeneous {
            schema: T::SCHEMA,
            meta: None,
            len: match NonZeroUsize::new(N) {
                Some(n) => n,
                None => panic!("Arrays must have at least one element"),
            },
        })),
    };
}

impl<T: TreeSerialize, const N: usize> TreeSerialize for [T; N]
where
    Self: TreeKey,
{
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        self[Self::SCHEMA.next(&mut keys)?].serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>, const N: usize> TreeDeserialize<'de> for [T; N] {
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        self[Self::SCHEMA.next(&mut keys)?].deserialize_by_key(keys, de)
    }

    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        Self::SCHEMA.next(&mut keys)?;
        T::probe_by_key(keys, de)
    }
}

impl<T: TreeAny, const N: usize> TreeAny for [T; N] {
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        self[Self::SCHEMA.next(&mut keys)?].ref_any_by_key(keys)
    }

    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        self[Self::SCHEMA.next(&mut keys)?].mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeKey> TreeKey for Option<T> {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSerialize> TreeSerialize for Option<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        self.as_ref()
            .ok_or(ValueError::Absent)?
            .serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Option<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        self.as_mut()
            .ok_or(ValueError::Absent)?
            .deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        T::probe_by_key(keys, de)
    }
}

impl<T: TreeAny> TreeAny for Option<T> {
    #[inline]
    fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
        self.as_ref()
            .ok_or(ValueError::Absent)?
            .ref_any_by_key(keys)
    }

    #[inline]
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        self.as_mut()
            .ok_or(ValueError::Absent)?
            .mut_any_by_key(keys)
    }
}
#[cfg(feature = "disabled")]
mod m {
    /////////////////////////////////////////////////////////////////////////////////////////

    const RESULT_LOOKUP: Internal = Internal::named(&["Ok", "Err"]);

    impl<T: TreeKey, E: TreeKey> TreeKey for Result<T, E> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            W::internal(&[T::traverse_all(), E::traverse_all()], &RESULT_LOOKUP)
        }

        #[inline]
        fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, SerDeError<G>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
        {
            match keys.next(&RESULT_LOOKUP)? {
                0 => T::traverse_by_key(keys, func),
                1 => E::traverse_by_key(keys, func),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
            .map(|depth| depth + 1)
        }
    }

    impl<T: TreeSerialize, E: TreeSerialize> TreeSerialize for Result<T, E> {
        #[inline]
        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            match (self, keys.next(&RESULT_LOOKUP)?) {
                (Ok(value), 0) => value.serialize_by_key(keys, ser),
                (Err(value), 1) => value.serialize_by_key(keys, ser),
                _ => Err(KeyError::Absent(0).into()),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<'de, T: TreeDeserialize<'de>, E: TreeDeserialize<'de>> TreeDeserialize<'de> for Result<T, E> {
        #[inline]
        fn deserialize_by_key<K, D>(
            &mut self,
            mut keys: K,
            de: D,
        ) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match (self, keys.next(&RESULT_LOOKUP)?) {
                (Ok(value), 0) => value.deserialize_by_key(keys, de),
                (Err(value), 1) => value.deserialize_by_key(keys, de),
                _ => Err(KeyError::Absent(0).into()),
            }
            .map_err(SerDeError::increment)
        }

        #[inline]
        fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RESULT_LOOKUP)? {
                0 => T::probe_by_key(keys, de),
                1 => E::probe_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<T: TreeAny, E: TreeAny> TreeAny for Result<T, E> {
        #[inline]
        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            match (self, keys.next(&RESULT_LOOKUP)?) {
                (Ok(value), 0) => value.ref_any_by_key(keys),
                (Err(value), 1) => value.ref_any_by_key(keys),
                _ => Err(KeyError::Absent(0)),
            }
            .map_err(KeyError::increment)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, KeyError>
        where
            K: Keys,
        {
            match (self, keys.next(&RESULT_LOOKUP)?) {
                (Ok(value), 0) => value.mut_any_by_key(keys),
                (Err(value), 1) => value.mut_any_by_key(keys),
                _ => Err(KeyError::Absent(0)),
            }
            .map_err(KeyError::increment)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    const BOUND_LOOKUP: Internal = Internal::named(&["Included", "Excluded"]);

    impl<T: TreeKey> TreeKey for Bound<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            W::internal(&[T::traverse_all(), T::traverse_all()], &BOUND_LOOKUP)
        }

        #[inline]
        fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, SerDeError<G>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
        {
            match keys.next(&BOUND_LOOKUP)? {
                0..=1 => T::traverse_by_key(keys, func),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
            .map(|depth| depth + 1)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for Bound<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            match (self, keys.next(&BOUND_LOOKUP)?) {
                (Self::Included(value), 0) | (Self::Excluded(value), 1) => {
                    value.serialize_by_key(keys, ser)
                }
                _ => Err(KeyError::Absent(0).into()),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Bound<T> {
        #[inline]
        fn deserialize_by_key<K, D>(
            &mut self,
            mut keys: K,
            de: D,
        ) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match (self, keys.next(&BOUND_LOOKUP)?) {
                (Self::Included(value), 0) | (Self::Excluded(value), 1) => {
                    value.deserialize_by_key(keys, de)
                }
                _ => Err(KeyError::Absent(0).into()),
            }
            .map_err(SerDeError::increment)
        }

        #[inline]
        fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RESULT_LOOKUP)? {
                0..=1 => T::probe_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<T: TreeAny> TreeAny for Bound<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            match (self, keys.next(&BOUND_LOOKUP)?) {
                (Self::Included(value), 0) | (Self::Excluded(value), 1) => {
                    value.ref_any_by_key(keys)
                }
                _ => Err(KeyError::Absent(0)),
            }
            .map_err(KeyError::increment)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, KeyError>
        where
            K: Keys,
        {
            match (self, keys.next(&BOUND_LOOKUP)?) {
                (Self::Included(value), 0) | (Self::Excluded(value), 1) => {
                    value.mut_any_by_key(keys)
                }
                _ => Err(KeyError::Absent(0)),
            }
            .map_err(KeyError::increment)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    const RANGE_LOOKUP: Internal = Internal::named(&["start", "end"]);

    impl<T: TreeKey> TreeKey for Range<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            W::internal(&[T::traverse_all(), T::traverse_all()], &RANGE_LOOKUP)
        }

        #[inline]
        fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, SerDeError<G>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0..=1 => T::traverse_by_key(keys, func),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
            .map(|depth| depth + 1)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for Range<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0 => self.start.serialize_by_key(keys, ser),
                1 => self.end.serialize_by_key(keys, ser),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Range<T> {
        #[inline]
        fn deserialize_by_key<K, D>(
            &mut self,
            mut keys: K,
            de: D,
        ) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0 => self.start.deserialize_by_key(keys, de),
                1 => self.end.deserialize_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }

        #[inline]
        fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RESULT_LOOKUP)? {
                0..=1 => T::probe_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<T: TreeAny> TreeAny for Range<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0 => self.start.ref_any_by_key(keys),
                1 => self.end.ref_any_by_key(keys),
                _ => unreachable!(),
            }
            .map_err(KeyError::increment)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, KeyError>
        where
            K: Keys,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0 => self.start.mut_any_by_key(keys),
                1 => self.end.mut_any_by_key(keys),
                _ => unreachable!(),
            }
            .map_err(KeyError::increment)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for RangeInclusive<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            W::internal(&[T::traverse_all(), T::traverse_all()], &RANGE_LOOKUP)
        }

        #[inline]
        fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, SerDeError<G>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0..=1 => T::traverse_by_key(keys, func),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
            .map(|depth| depth + 1)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for RangeInclusive<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            match keys.next(&RANGE_LOOKUP)? {
                0 => self.start().serialize_by_key(keys, ser),
                1 => self.end().serialize_by_key(keys, ser),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    const RANGE_FROM_LOOKUP: Internal = Internal::named(&["start"]);

    impl<T: TreeKey> TreeKey for RangeFrom<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            W::internal(&[T::traverse_all()], &RANGE_FROM_LOOKUP)
        }

        #[inline]
        fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, SerDeError<G>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
        {
            match keys.next(&RANGE_FROM_LOOKUP)? {
                0 => T::traverse_by_key(keys, func),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
            .map(|depth| depth + 1)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for RangeFrom<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            match keys.next(&RANGE_FROM_LOOKUP)? {
                0 => self.start.serialize_by_key(keys, ser),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RangeFrom<T> {
        #[inline]
        fn deserialize_by_key<K, D>(
            &mut self,
            mut keys: K,
            de: D,
        ) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RANGE_FROM_LOOKUP)? {
                0 => self.start.deserialize_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }

        #[inline]
        fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RESULT_LOOKUP)? {
                0 => T::probe_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<T: TreeAny> TreeAny for RangeFrom<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            match keys.next(&RANGE_FROM_LOOKUP)? {
                0 => self.start.ref_any_by_key(keys),
                _ => unreachable!(),
            }
            .map_err(KeyError::increment)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, KeyError>
        where
            K: Keys,
        {
            match keys.next(&RANGE_FROM_LOOKUP)? {
                0 => self.start.mut_any_by_key(keys),
                _ => unreachable!(),
            }
            .map_err(KeyError::increment)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    const RANGE_TO_LOOKUP: Internal = Internal::named(&["end"]);

    impl<T: TreeKey> TreeKey for RangeTo<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            W::internal(&[T::traverse_all()], &RANGE_TO_LOOKUP)
        }

        #[inline]
        fn traverse_by_key<K, F, G>(mut keys: K, func: F) -> Result<usize, SerDeError<G>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), G>,
        {
            match keys.next(&RANGE_TO_LOOKUP)? {
                0 => T::traverse_by_key(keys, func),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
            .map(|depth| depth + 1)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for RangeTo<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            match keys.next(&RANGE_TO_LOOKUP)? {
                0 => self.end.serialize_by_key(keys, ser),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RangeTo<T> {
        #[inline]
        fn deserialize_by_key<K, D>(
            &mut self,
            mut keys: K,
            de: D,
        ) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RANGE_TO_LOOKUP)? {
                0 => self.end.deserialize_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }

        #[inline]
        fn probe_by_key<K, D>(mut keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            match keys.next(&RESULT_LOOKUP)? {
                0 => T::probe_by_key(keys, de),
                _ => unreachable!(),
            }
            .map_err(SerDeError::increment)
        }
    }

    impl<T: TreeAny> TreeAny for RangeTo<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            match keys.next(&RANGE_TO_LOOKUP)? {
                0 => self.end.ref_any_by_key(keys),
                _ => unreachable!(),
            }
            .map_err(KeyError::increment)
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, KeyError>
        where
            K: Keys,
        {
            match keys.next(&RANGE_TO_LOOKUP)? {
                0 => self.end.mut_any_by_key(keys),
                _ => unreachable!(),
            }
            .map_err(KeyError::increment)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for Cell<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize + Copy> TreeSerialize for Cell<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            self.get().serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Cell<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.get_mut().deserialize_by_key(keys, de)
        }

        #[inline]
        fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            T::probe_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for Cell<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            Err(KeyError::Access(0, "Can't leak out of Cell"))
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
        where
            K: Keys,
        {
            self.get_mut().mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeKey> TreeKey for RefCell<T> {
        #[inline]
        fn traverse_all<W: Walk>() -> W {
            T::traverse_all()
        }

        #[inline]
        fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
        {
            T::traverse_by_key(keys, func)
        }
    }

    impl<T: TreeSerialize> TreeSerialize for RefCell<T> {
        #[inline]
        fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
        where
            K: Keys,
            S: Serializer,
        {
            self.try_borrow()
                .or(Err(KeyError::Access(0, "Borrowed")))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RefCell<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.get_mut().deserialize_by_key(keys, de)
        }

        #[inline]
        fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            T::probe_by_key(keys, de)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &RefCell<T> {
        #[inline]
        fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            self.try_borrow_mut()
                .or(Err(KeyError::Access(0, "Borrowed")))?
                .deserialize_by_key(keys, de)
        }

        #[inline]
        fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
        where
            K: Keys,
            D: Deserializer<'de>,
        {
            T::probe_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for RefCell<T> {
        #[inline]
        fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, KeyError>
        where
            K: Keys,
        {
            Err(KeyError::Access(0, "Can't leak out of RefCell"))
        }

        #[inline]
        fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
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
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for Box<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                (**self).serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Box<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                (**self).deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<T: TreeAny> TreeAny for Box<T> {
            #[inline]
            fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, KeyError>
            where
                K: Keys,
            {
                (**self).ref_any_by_key(keys)
            }

            #[inline]
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
            where
                K: Keys,
            {
                (**self).mut_any_by_key(keys)
            }
        }

        /////////////////////////////////////////////////////////////////////////////////////////

        impl<T: TreeKey + Clone> TreeKey for Cow<'_, T> {
            #[inline]
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize + Clone> TreeSerialize for Cow<'_, T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                (**self).serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de> + Clone> TreeDeserialize<'de> for Cow<'_, T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self.to_mut().deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<T: TreeAny + Clone> TreeAny for Cow<'_, T> {
            #[inline]
            fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, KeyError>
            where
                K: Keys,
            {
                (**self).ref_any_by_key(keys)
            }

            #[inline]
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
            where
                K: Keys,
            {
                self.to_mut().mut_any_by_key(keys)
            }
        }

        /////////////////////////////////////////////////////////////////////////////////////////

        impl<T: TreeKey> TreeKey for Rc<T> {
            #[inline]
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for Rc<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                (**self).serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Rc<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                Rc::get_mut(self)
                    .ok_or(KeyError::Access(0, "Reference is taken"))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<T: TreeAny> TreeAny for Rc<T> {
            #[inline]
            fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, KeyError>
            where
                K: Keys,
            {
                (**self).ref_any_by_key(keys)
            }

            #[inline]
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
            where
                K: Keys,
            {
                Rc::get_mut(self)
                    .ok_or(KeyError::Access(0, "Reference is taken"))?
                    .mut_any_by_key(keys)
            }
        }

        /////////////////////////////////////////////////////////////////////////////////////////

        impl<T: TreeKey> TreeKey for rc::Weak<T> {
            #[inline]
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for rc::Weak<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                self.upgrade()
                    .ok_or(KeyError::Absent(0))?
                    .serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for rc::Weak<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self.upgrade()
                    .ok_or(KeyError::Absent(0))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        /////////////////////////////////////////////////////////////////////////////////////////

        impl<T: TreeKey> TreeKey for Arc<T> {
            #[inline]
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for Arc<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                (**self).serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Arc<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                Arc::get_mut(self)
                    .ok_or(KeyError::Access(0, "Reference is taken"))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<T: TreeAny> TreeAny for Arc<T> {
            #[inline]
            fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, KeyError>
            where
                K: Keys,
            {
                (**self).ref_any_by_key(keys)
            }

            #[inline]
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
            where
                K: Keys,
            {
                Arc::get_mut(self)
                    .ok_or(KeyError::Access(0, "Reference is taken"))?
                    .mut_any_by_key(keys)
            }
        }

        /////////////////////////////////////////////////////////////////////////////////////////

        impl<T: TreeKey> TreeKey for sync::Weak<T> {
            #[inline]
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for sync::Weak<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                self.upgrade()
                    .ok_or(KeyError::Absent(0))?
                    .serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for sync::Weak<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self.upgrade()
                    .ok_or(KeyError::Absent(0))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
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
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for Mutex<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                self.lock()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Mutex<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self.get_mut()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &Mutex<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                (*self)
                    .lock()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<T: TreeAny> TreeAny for Mutex<T> {
            #[inline]
            fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, KeyError>
            where
                K: Keys,
            {
                Err(KeyError::Access(0, "Can't leak out of Mutex"))
            }

            #[inline]
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
            where
                K: Keys,
            {
                self.get_mut()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .mut_any_by_key(keys)
            }
        }

        /////////////////////////////////////////////////////////////////////////////////////////

        impl<T: TreeKey> TreeKey for RwLock<T> {
            #[inline]
            fn traverse_all<W: Walk>() -> W {
                T::traverse_all()
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, SerDeError<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, NonZero<usize>) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }
        }

        impl<T: TreeSerialize> TreeSerialize for RwLock<T> {
            #[inline]
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<S::Ok, SerDeError<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                self.read()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .serialize_by_key(keys, ser)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &RwLock<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self.write()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RwLock<T> {
            #[inline]
            fn deserialize_by_key<K, D>(
                &mut self,
                keys: K,
                de: D,
            ) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                self.get_mut()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .deserialize_by_key(keys, de)
            }

            #[inline]
            fn probe_by_key<K, D>(keys: K, de: D) -> Result<(), SerDeError<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                T::probe_by_key(keys, de)
            }
        }

        impl<T: TreeAny> TreeAny for RwLock<T> {
            #[inline]
            fn ref_any_by_key<K>(&self, _keys: K) -> Result<&dyn Any, KeyError>
            where
                K: Keys,
            {
                Err(KeyError::Access(0, "Can't leak out of RwLock"))
            }

            #[inline]
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, KeyError>
            where
                K: Keys,
            {
                self.get_mut()
                    .or(Err(KeyError::Access(0, "Poisoned")))?
                    .mut_any_by_key(keys)
            }
        }
    }
}
