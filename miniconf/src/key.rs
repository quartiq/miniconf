use crate::{Error, TreeKey};

/// Capability to convert a key into a node index for a given `M: TreeKey`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find<const Y: usize, M: TreeKey<Y>>(&self) -> Option<usize>;
}

// `usize` index as Key
impl Key for usize {
    fn find<const Y: usize, M>(&self) -> Option<usize> {
        Some(*self)
    }
}

// &str name as Key
impl Key for &str {
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

    /// Look up a key in a [`TreeKey`] and convert to `usize` index.
    ///
    /// # Args
    /// * `len` as for [`Keys::next()`]
    fn lookup<const Y: usize, M: TreeKey<Y>, E>(&mut self) -> Result<usize, Error<E>> {
        self.next(M::len())
            .ok_or(Error::TooShort(0))?
            .find::<Y, M>()
            .ok_or(Error::NotFound(1))
    }

    /// Return whether there are more keys.
    ///
    /// This may mutate and consume remaining keys.
    #[inline]
    fn is_empty(&mut self) -> bool {
        self.next(0).is_none()
    }
}

impl<T> Keys for T
where
    T: Iterator,
    T::Item: Key,
{
    type Item = T::Item;

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

    fn into_keys(self) -> Self::IntoKeys {
        self.into_iter()
    }
}
