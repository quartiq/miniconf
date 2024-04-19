use crate::{Error, Keys, Metadata, TreeKey};
use core::marker::PhantomData;

impl<T: TreeKey<Y>, const Y: usize> TreeKey<Y> for PhantomData<T> {
    #[inline]
    fn len() -> usize {
        T::len()
    }

    #[inline]
    fn name_to_index(value: &str) -> Option<usize> {
        T::name_to_index(value)
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
