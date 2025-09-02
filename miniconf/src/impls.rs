use core::any::Any;
use core::cell::{Cell, RefCell};
use core::ops::{Bound, Range, RangeFrom, RangeInclusive, RangeTo};

use serde::{Deserializer, Serializer};

use crate::{
    Homogeneous, Keys, Named, Numbered, Schema, SerDeError, TreeAny, TreeDeserialize, TreeSchema,
    TreeSerialize, ValueError,
};

/////////////////////////////////////////////////////////////////////////////////////////

macro_rules! impl_tuple {
    ($($i:tt $t:ident)+) => {
        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeSchema),+> TreeSchema for ($($t,)+) {
            const SCHEMA: &'static Schema = &Schema::numbered(&[$(
                Numbered::new($t::SCHEMA),
            )+]);
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeSerialize),+> TreeSerialize for ($($t,)+) {
            #[inline]
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
            #[inline]
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

            #[inline]
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
            #[inline]
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

            #[inline]
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
impl_tuple!(0 T0);
impl_tuple!(0 T0 1 T1);
impl_tuple!(0 T0 1 T1 2 T2);
impl_tuple!(0 T0 1 T1 2 T2 3 T3);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4 5 T5);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7);

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema, const N: usize> TreeSchema for [T; N] {
    const SCHEMA: &'static Schema = &Schema::homogeneous(Homogeneous::new(N, T::SCHEMA));
}

impl<T: TreeSerialize, const N: usize> TreeSerialize for [T; N]
where
    Self: TreeSchema,
{
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        self[Self::SCHEMA.next(&mut keys)?].serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>, const N: usize> TreeDeserialize<'de> for [T; N] {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        self[Self::SCHEMA.next(&mut keys)?].deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        Self::SCHEMA.next(&mut keys)?;
        T::probe_by_key(keys, de)
    }
}

impl<T: TreeAny, const N: usize> TreeAny for [T; N] {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        self[Self::SCHEMA.next(&mut keys)?].ref_any_by_key(keys)
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        self[Self::SCHEMA.next(&mut keys)?].mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for Option<T> {
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

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema, E: TreeSchema> TreeSchema for Result<T, E> {
    const SCHEMA: &'static Schema =
        &Schema::named(&[Named::new("Ok", T::SCHEMA), Named::new("Err", E::SCHEMA)]);
}

impl<T: TreeSerialize, E: TreeSerialize> TreeSerialize for Result<T, E> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Ok(value), 0) => value.serialize_by_key(keys, ser),
            (Err(value), 1) => value.serialize_by_key(keys, ser),
            _ => Err(ValueError::Absent.into()),
        }
    }
}

impl<'de, T: TreeDeserialize<'de>, E: TreeDeserialize<'de>> TreeDeserialize<'de> for Result<T, E> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Ok(value), 0) => value.deserialize_by_key(keys, de),
            (Err(value), 1) => value.deserialize_by_key(keys, de),
            _ => Err(ValueError::Absent.into()),
        }
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => T::probe_by_key(keys, de),
            1 => E::probe_by_key(keys, de),
            _ => unreachable!(),
        }
    }
}

impl<T: TreeAny, E: TreeAny> TreeAny for Result<T, E> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Ok(value), 0) => value.ref_any_by_key(keys),
            (Err(value), 1) => value.ref_any_by_key(keys),
            _ => Err(ValueError::Absent),
        }
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Ok(value), 0) => value.mut_any_by_key(keys),
            (Err(value), 1) => value.mut_any_by_key(keys),
            _ => Err(ValueError::Absent),
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for Bound<T> {
    const SCHEMA: &'static Schema = &Schema::named(&[
        Named::new("Included", T::SCHEMA),
        Named::new("Excluded", T::SCHEMA),
    ]);
}

impl<T: TreeSerialize> TreeSerialize for Bound<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Self::Included(value), 0) | (Self::Excluded(value), 1) => {
                value.serialize_by_key(keys, ser)
            }
            _ => Err(ValueError::Absent.into()),
        }
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Bound<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Self::Included(value), 0) | (Self::Excluded(value), 1) => {
                value.deserialize_by_key(keys, de)
            }
            _ => Err(ValueError::Absent.into()),
        }
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0..=1 => T::probe_by_key(keys, de),
            _ => unreachable!(),
        }
    }
}

impl<T: TreeAny> TreeAny for Bound<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Self::Included(value), 0) | (Self::Excluded(value), 1) => value.ref_any_by_key(keys),
            _ => Err(ValueError::Absent),
        }
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        match (self, Self::SCHEMA.next(&mut keys)?) {
            (Self::Included(value), 0) | (Self::Excluded(value), 1) => value.mut_any_by_key(keys),
            _ => Err(ValueError::Absent),
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for Range<T> {
    const SCHEMA: &'static Schema =
        &Schema::named(&[Named::new("start", T::SCHEMA), Named::new("end", T::SCHEMA)]);
}

impl<T: TreeSerialize> TreeSerialize for Range<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => &self.start,
            1 => &self.end,
            _ => unreachable!(),
        }
        .serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Range<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => &mut self.start,
            1 => &mut self.end,
            _ => unreachable!(),
        }
        .deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0..=1 => T::probe_by_key(keys, de),
            _ => unreachable!(),
        }
    }
}

impl<T: TreeAny> TreeAny for Range<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => &self.start,
            1 => &self.end,
            _ => unreachable!(),
        }
        .ref_any_by_key(keys)
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => &mut self.start,
            1 => &mut self.end,
            _ => unreachable!(),
        }
        .mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for RangeInclusive<T> {
    const SCHEMA: &'static Schema = Range::<T>::SCHEMA;
}

impl<T: TreeSerialize> TreeSerialize for RangeInclusive<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.start(),
            1 => self.end(),
            _ => unreachable!(),
        }
        .serialize_by_key(keys, ser)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for RangeFrom<T> {
    const SCHEMA: &'static Schema = &Schema::named(&[Named::new("start", T::SCHEMA)]);
}

impl<T: TreeSerialize> TreeSerialize for RangeFrom<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.start.serialize_by_key(keys, ser),
            _ => unreachable!(),
        }
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RangeFrom<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.start.deserialize_by_key(keys, de),
            _ => unreachable!(),
        }
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => T::probe_by_key(keys, de),
            _ => unreachable!(),
        }
    }
}

impl<T: TreeAny> TreeAny for RangeFrom<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.start.ref_any_by_key(keys),
            _ => unreachable!(),
        }
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.start.mut_any_by_key(keys),
            _ => unreachable!(),
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for RangeTo<T> {
    const SCHEMA: &'static Schema = &Schema::named(&[Named::new("end", T::SCHEMA)]);
}

impl<T: TreeSerialize> TreeSerialize for RangeTo<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.end.serialize_by_key(keys, ser),
            _ => unreachable!(),
        }
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RangeTo<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.end.deserialize_by_key(keys, de),
            _ => unreachable!(),
        }
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => T::probe_by_key(keys, de),
            _ => unreachable!(),
        }
    }
}

impl<T: TreeAny> TreeAny for RangeTo<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.end.ref_any_by_key(keys),
            _ => unreachable!(),
        }
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        match Self::SCHEMA.next(&mut keys)? {
            0 => self.end.mut_any_by_key(keys),
            _ => unreachable!(),
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for Cell<T> {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSerialize + Copy> TreeSerialize for Cell<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        self.get().serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Cell<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        self.get_mut().deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        T::probe_by_key(keys, de)
    }
}

impl<T: TreeAny> TreeAny for Cell<T> {
    #[inline]
    fn ref_any_by_key(&self, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
        Err(ValueError::Access("Can't leak out of Cell"))
    }

    #[inline]
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        self.get_mut().mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

impl<T: TreeSchema> TreeSchema for RefCell<T> {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSerialize> TreeSerialize for RefCell<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        self.try_borrow()
            .or(Err(ValueError::Access("Borrowed")))?
            .serialize_by_key(keys, ser)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RefCell<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        self.get_mut().deserialize_by_key(keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        T::probe_by_key(keys, de)
    }
}

impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &RefCell<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
        self.try_borrow_mut()
            .or(Err(ValueError::Access("Borrowed")))?
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

impl<T: TreeAny> TreeAny for RefCell<T> {
    #[inline]
    fn ref_any_by_key(&self, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
        Err(ValueError::Access("Can't leak out of RefCell"))
    }

    #[inline]
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        self.get_mut().mut_any_by_key(keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

#[cfg(feature = "alloc")]
mod _alloc {
    use super::*;
    extern crate alloc;
    use alloc::{
        borrow::Cow,
        boxed::Box,
        rc::{Rc, Weak as RcWeak},
        sync::{Arc, Weak as SyncWeak},
    };

    impl<T: TreeSchema> TreeSchema for Box<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for Box<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Box<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            (**self).deserialize_by_key(keys, de)
        }

        #[inline]
        fn probe_by_key<D: Deserializer<'de>>(
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            T::probe_by_key(keys, de)
        }
    }

    impl<T: TreeAny> TreeAny for Box<T> {
        #[inline]
        fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            (**self).mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeSchema + Clone> TreeSchema for Cow<'_, T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize + Clone> TreeSerialize for Cow<'_, T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de> + Clone> TreeDeserialize<'de> for Cow<'_, T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            self.to_mut().deserialize_by_key(keys, de)
        }

        #[inline]
        fn probe_by_key<D: Deserializer<'de>>(
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            T::probe_by_key(keys, de)
        }
    }

    impl<T: TreeAny + Clone> TreeAny for Cow<'_, T> {
        #[inline]
        fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            self.to_mut().mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeSchema> TreeSchema for Rc<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for Rc<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Rc<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            Rc::get_mut(self)
                .ok_or(ValueError::Access("Reference is taken"))?
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

    impl<T: TreeAny> TreeAny for Rc<T> {
        #[inline]
        fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            Rc::get_mut(self)
                .ok_or(ValueError::Access("Reference is taken"))?
                .mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeSchema> TreeSchema for RcWeak<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for RcWeak<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            self.upgrade()
                .ok_or(ValueError::Absent)?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RcWeak<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            self.upgrade()
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

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeSchema> TreeSchema for Arc<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for Arc<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            (**self).serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Arc<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            Arc::get_mut(self)
                .ok_or(ValueError::Access("Reference is taken"))?
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

    impl<T: TreeAny> TreeAny for Arc<T> {
        #[inline]
        fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
            (**self).ref_any_by_key(keys)
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            Arc::get_mut(self)
                .ok_or(ValueError::Access("Reference is taken"))?
                .mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeSchema> TreeSchema for SyncWeak<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for SyncWeak<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            self.upgrade()
                .ok_or(ValueError::Absent)?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for SyncWeak<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            self.upgrade()
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
}

/////////////////////////////////////////////////////////////////////////////////////////

#[cfg(feature = "std")]
mod _std {
    use super::*;
    use std::sync::{Mutex, RwLock};

    impl<T: TreeSchema> TreeSchema for Mutex<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for Mutex<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            self.lock()
                .or(Err(ValueError::Access("Poisoned")))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for Mutex<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            self.get_mut()
                .or(Err(ValueError::Access("Poisoned")))?
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

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &Mutex<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            (*self)
                .lock()
                .or(Err(ValueError::Access("Poisoned")))?
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

    impl<T: TreeAny> TreeAny for Mutex<T> {
        #[inline]
        fn ref_any_by_key(&self, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
            Err(ValueError::Access("Can't leak out of Mutex"))
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            self.get_mut()
                .or(Err(ValueError::Access("Poisoned")))?
                .mut_any_by_key(keys)
        }
    }

    /////////////////////////////////////////////////////////////////////////////////////////

    impl<T: TreeSchema> TreeSchema for RwLock<T> {
        const SCHEMA: &'static Schema = T::SCHEMA;
    }

    impl<T: TreeSerialize> TreeSerialize for RwLock<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerDeError<S::Error>> {
            self.read()
                .or(Err(ValueError::Access("Poisoned")))?
                .serialize_by_key(keys, ser)
        }
    }

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for &RwLock<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            self.write()
                .or(Err(ValueError::Access("Poisoned")))?
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

    impl<'de, T: TreeDeserialize<'de>> TreeDeserialize<'de> for RwLock<T> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerDeError<D::Error>> {
            self.get_mut()
                .or(Err(ValueError::Access("Poisoned")))?
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

    impl<T: TreeAny> TreeAny for RwLock<T> {
        #[inline]
        fn ref_any_by_key(&self, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
            Err(ValueError::Access("Can't leak out of RwLock"))
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            self.get_mut()
                .or(Err(ValueError::Access("Poisoned")))?
                .mut_any_by_key(keys)
        }
    }
}
