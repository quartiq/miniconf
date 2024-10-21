use core::any::Any;

use serde::{de::Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Error, KeyLookup, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

macro_rules! impl_tuple {
    ($($i:tt $t:ident)+) => {
        impl<$($t: TreeKey),+> TreeKey for ($($t),+) {
            fn traverse_all<W: Walk>() -> Result<W, W::Error> {
                unimplemented!()
                //T::traverse_all()
            }
        
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                unimplemented!()
                //T::traverse_by_key(keys, func)
            }
        }
        
        impl<$($t: TreeSerialize),+> TreeSerialize for ($($t),+) {
            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Keys,
                S: Serializer,
            {
                unimplemented!()
                // inner.serialize_by_key(keys, ser)
            }
        }
        
        impl<'de, $($t: TreeDeserialize<'de>),+> TreeDeserialize<'de> for ($($t),+) {
            fn deserialize_by_key<K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Keys,
                D: Deserializer<'de>,
            {
                unimplemented!()
                // inner.deserialize_by_key(keys, de)
            }
        }
        
        impl<$($t: TreeAny),+> TreeAny for ($($t),+) {
            fn ref_any_by_key<K>(&self, keys: K) -> Result<&dyn Any, Traversal>
            where
                K: Keys,
            {
                unimplemented!()
                // inner.ref_any_by_key(keys)
            }
        
            fn mut_any_by_key<K>(&mut self, keys: K) -> Result<&mut dyn Any, Traversal>
            where
                K: Keys,
            {
                unimplemented!()
                // inner.mut_any_by_key(keys)
            }
        }
    }
}
// impl_tuple!(T0 0);
impl_tuple!(0 T0 1 T1);
