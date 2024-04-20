use crate::{Error, Keys, Metadata, TreeKey};
use core::marker::PhantomData;

macro_rules! depth {
    ($($y:literal)+) => {$(
        impl<T: TreeKey<{$y - 1}>> TreeKey<$y> for PhantomData<T> {
            #[inline]
            fn len() -> usize {
                0
            }

            #[inline]
            fn name_to_index(_value: &str) -> Option<usize> {
                None
            }

            #[inline]
            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
            where
                K: Keys,
                F: FnMut(usize, &str, usize) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }

            #[inline]
            fn metadata() -> Metadata {
                T::metadata()
            }
        }
    )+}
}
depth!(2 3 4 5 6 7 8);
