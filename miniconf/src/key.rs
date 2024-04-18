use crate::TreeKey;

/// Capability to convert a key into a node index for a given `M: TreeKey`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find<const Y: usize, M: TreeKey<Y>>(&self) -> Option<usize>;
}

// `usize` index as Key
impl Key for usize {
    #[inline]
    fn find<const Y: usize, M>(&self) -> Option<usize> {
        Some(*self)
    }
}

// &str name as Key
impl Key for &str {
    #[inline]
    fn find<const Y: usize, M: TreeKey<Y>>(&self) -> Option<usize> {
        M::name_to_index(self)
    }
}

/// Capability to yield keys given `M: TreeKey`
pub trait Keys {
    /// The type of key that we yield.
    type Item: Key;

    /// Convert the next key `self` to a `usize` index.
    ///
    /// # Args
    /// * `len` is an upper limit to the number of keys at this level.
    ///   It is non-zero.
    fn next(&mut self, len: usize) -> Option<Self::Item>;
}

impl<T> Keys for T
where
    T: Iterator,
    T::Item: Key,
{
    type Item = T::Item;

    #[inline]
    fn next(&mut self, _len: usize) -> Option<Self::Item> {
        Iterator::next(self)
    }
}

/// Capability to be converted into a [`Keys`]
pub trait IntoKeys {
    /// The specific [`Keys`] implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a [`Keys`] implementor.
    fn into_keys(self) -> Self::IntoKeys;
}

impl<T> IntoKeys for T
where
    T: IntoIterator,
    T::IntoIter: Keys,
{
    type IntoKeys = T::IntoIter;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self.into_iter()
    }
}
