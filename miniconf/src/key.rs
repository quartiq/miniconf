use crate::{Traversal, TreeKey};

/// Capability to convert a key into a node index for a given `M: TreeKey`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find<const Y: usize, M: TreeKey<Y> + ?Sized>(&self) -> Option<usize>;
}

// `usize` index as Key
impl Key for usize {
    fn find<const Y: usize, M: ?Sized>(&self) -> Option<usize> {
        Some(*self)
    }
}

// &str name as Key
impl Key for &str {
    fn find<const Y: usize, M: TreeKey<Y> + ?Sized>(&self) -> Option<usize> {
        M::name_to_index(self)
    }
}

/// Capability to yield [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`TreeKey`] and convert to `usize` index.
    fn next<const Y: usize, M: TreeKey<Y> + ?Sized>(&mut self) -> Result<usize, Traversal>;

    /// Return whether there are more keys.
    ///
    /// This may mutate and consume remaining keys.
    fn is_empty(&mut self) -> bool;

    /// Check for no remaining keys.
    fn finalize<const Y: usize>(&mut self) -> Result<(), Traversal> {
        if !self.is_empty() {
            Err(Traversal::TooLong(Y))
        } else {
            Ok(())
        }
    }
}

impl<T> Keys for T
where
    T: Iterator,
    T::Item: Key,
{
    fn next<const Y: usize, M: TreeKey<Y> + ?Sized>(&mut self) -> Result<usize, Traversal> {
        let key = Iterator::next(self).ok_or(Traversal::TooShort(0))?;
        key.find::<Y, M>().ok_or(Traversal::NotFound(1))
    }

    fn is_empty(&mut self) -> bool {
        self.next().is_none()
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
