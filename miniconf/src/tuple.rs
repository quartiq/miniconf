use core::any::Any;

use serde::{Deserializer, Serializer};

use crate::{
    Error, KeyLookup, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

macro_rules! count_tts {
    () => {0usize};
    ($_head:tt $($tail:tt)*) => {1usize + count_tts!($($tail)*)};
}

macro_rules! impl_tuple {
    ($($i:tt $t:ident)*) => {
        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeKey),*> TreeKey for ($($t,)*) {
            fn traverse_all<W: Walk>() -> Result<W, W::Error> {
                let mut walk = W::internal();
                let k = KeyLookup::homogeneous(count_tts!($($t)*));
                $(walk = walk.merge(&$t::traverse_all::<W>()?, Some($i), &k)?;)*
                Ok(walk)
            }

            fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                let k = KeyLookup::homogeneous(count_tts!($($t)*));
                let index = keys.next(&k)?;
                if index >= k.len {
                    Err(Traversal::NotFound(1))?
                }
                func(index, None, k.len).map_err(|err| Error::Inner(1, err))?;
                Error::increment_result(match index {
                    $($i => $t::traverse_by_key(keys, func),)*
                    _ => unreachable!()
                })
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeSerialize),*> TreeSerialize for ($($t,)*) {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                let k = KeyLookup::homogeneous(count_tts!($($t)*));
                let index = keys.next(&k)?;
                if index >= k.len {
                    Err(Traversal::NotFound(1))?
                }
                Error::increment_result(match index {
                    $($i => $t::serialize_by_key(&self.$i, keys, ser),)*
                    _ => unreachable!()
                })
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<'de, $($t: TreeDeserialize<'de>),*> TreeDeserialize<'de> for ($($t,)*) {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                let k = KeyLookup::homogeneous(count_tts!($($t)*));
                let index = keys.next(&k)?;
                if index >= k.len {
                    Err(Traversal::NotFound(1))?
                }
                Error::increment_result(match index {
                    $($i => $t::deserialize_by_key(&mut self.$i, keys, de),)*
                    _ => unreachable!()
                })
            }
        }

        #[allow(unreachable_code, unused_mut, unused)]
        impl<$($t: TreeAny),*> TreeAny for ($($t,)*) {
            fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
            where
                K: Keys,
            {
                let k = KeyLookup::homogeneous(count_tts!($($t)*));
                let index = keys.next(&k)?;
                if index >= k.len {
                    Err(Traversal::NotFound(1))?
                }
                let ret: Result<_, _> = match index {
                    $($i => $t::ref_any_by_key(&self.$i, keys),)*
                    _ => unreachable!()
                };
                ret.map_err(Traversal::increment)
            }

            fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
            where
                K: Keys,
            {
                let k = KeyLookup::homogeneous(count_tts!($($t)*));
                let index = keys.next(&k)?;
                if index >= k.len {
                    Err(Traversal::NotFound(1))?
                }
                let ret: Result<_, _> = match index {
                    $($i => $t::mut_any_by_key(&mut self.$i, keys),)*
                    _ => unreachable!()
                };
                ret.map_err(Traversal::increment)
            }
        }
    }
}
impl_tuple!();
impl_tuple!(0 T0);
impl_tuple!(0 T0 1 T1);
impl_tuple!(0 T0 1 T1 2 T2);
impl_tuple!(0 T0 1 T1 2 T2 3 T3);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4 5 T5);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6);
impl_tuple!(0 T0 1 T1 2 T2 3 T3 4 T4 5 T5 6 T6 7 T7);
