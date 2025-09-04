use core::{convert::Infallible, iter::Fuse};

use serde::Serialize;

use crate::{DescendError, Internal, KeyError, Schema};

/// Convert a key into a node index given an internal node schema
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find(&self, internal: &Internal) -> Option<usize>;
}

impl<T: Key> Key for &T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, internal: &Internal) -> Option<usize> {
        (**self).find(internal)
    }
}

impl<T: Key> Key for &mut T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, internal: &Internal) -> Option<usize> {
        (**self).find(internal)
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys: Sized {
    /// Look up the next key in a [`Internal`] and convert to `usize` index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError>;

    /// Finalize the keys, ensure there are no more.
    ///
    /// This must be fused.
    fn finalize(&mut self) -> Result<(), KeyError>;

    /// Chain another `Keys` to this one.
    #[inline]
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self, U::IntoKeys> {
        Chain(self, other.into_keys())
    }

    /// Track consumption
    #[inline]
    fn track(self) -> Track<Self> {
        Track {
            inner: self,
            depth: 0,
        }
    }

    #[inline]
    fn short(self) -> Short<Self> {
        Short {
            inner: self,
            leaf: false,
        }
    }
}

impl<T> Keys for &mut T
where
    T: Keys + ?Sized,
{
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        (**self).next(internal)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        (**self).finalize()
    }
}

/// Be converted into a `Keys`
pub trait IntoKeys {
    /// The specific `Keys` implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a `Keys` implementor.
    fn into_keys(self) -> Self::IntoKeys;
}

/// Look up an `IntoKeys` in a `TreeSchema` and transcode it.
pub trait Transcode {
    type Error;
    /// Perform a node lookup of a `K: IntoKeys` on a `M: TreeSchema` and transcode it.
    ///
    /// This modifies `self` such that afterwards `Self: IntoKeys` can be used on `M` again.
    /// It returns a `Node` with node type and depth information.
    ///
    /// Returning `Err(Traversal::Absent)` indicates that there was insufficient
    /// capacity and a key could not be encoded at the given depth.
    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>>;
}

impl<T: Transcode + ?Sized> Transcode for &mut T {
    type Error = T::Error;
    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        (**self).transcode(schema, keys)
    }
}

#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Short<K> {
    pub inner: K,
    pub leaf: bool,
}

impl<K> Short<K> {
    pub fn new(inner: K) -> Self {
        Self { inner, leaf: false }
    }
}

impl<K: Keys> IntoKeys for &mut Short<K> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self.leaf = false;
        self
    }
}

impl<K: Keys> Keys for Short<K> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        self.inner.next(internal)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        self.inner.finalize()?;
        self.leaf = true;
        Ok(())
    }
}

impl<T: Transcode> Transcode for Short<T> {
    type Error = T::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        self.leaf = false;
        match self.inner.transcode(schema, keys) {
            Err(DescendError::Key(KeyError::TooShort)) => Ok(()),
            Ok(()) | Err(DescendError::Key(KeyError::TooLong)) => {
                self.leaf = true;
                Ok(())
            }
            ret => ret,
        }
    }
}

/// Track keys consumption and leaf encounter
#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Track<K> {
    pub inner: K,
    pub depth: usize,
}

impl<K> Track<K> {
    pub fn new(inner: K) -> Self {
        Self { inner, depth: 0 }
    }
}

impl<K: Keys> IntoKeys for &mut Track<K> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self.depth = 0;
        self
    }
}

impl<K: Keys> Keys for Track<K> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let k = self.inner.next(internal);
        if k.is_ok() {
            self.depth += 1;
        }
        k
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        self.inner.finalize()
    }
}

impl<T: Transcode> Transcode for Track<T> {
    type Error = T::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        self.depth = 0;
        let mut tracked = keys.into_keys().track();
        let ret = self.inner.transcode(schema, &mut tracked);
        self.depth = tracked.depth;
        ret
    }
}

/// Shim to provide the bare lookup/Track/Short without transcoding target
impl Transcode for () {
    type Error = Infallible;
    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_, _| Ok(()))
    }
}

/// [`Keys`]/[`IntoKeys`] for Iterators of [`Key`]
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct KeysIter<T>(Fuse<T>);

impl<T: Iterator> KeysIter<T> {
    #[inline]
    fn new(inner: T) -> Self {
        Self(inner.fuse())
    }
}

impl<T> Keys for KeysIter<T>
where
    T: Iterator,
    T::Item: Key,
{
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let n = self.0.next().ok_or(KeyError::TooShort)?;
        n.find(internal).ok_or(KeyError::NotFound)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        match self.0.next() {
            Some(_) => Err(KeyError::TooLong),
            None => Ok(()),
        }
    }
}

impl<T> IntoKeys for T
where
    T: IntoIterator,
    <T::IntoIter as Iterator>::Item: Key,
{
    type IntoKeys = KeysIter<T::IntoIter>;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        KeysIter::new(self.into_iter())
    }
}

impl<T> IntoKeys for KeysIter<T>
where
    T: Iterator,
    T::Item: Key,
{
    type IntoKeys = KeysIter<T>;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

/// Concatenate two `Keys` of different types
pub struct Chain<T, U>(T, U);

impl<T: Keys, U: Keys> Keys for Chain<T, U> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        match self.0.next(internal) {
            Err(KeyError::TooShort) => self.1.next(internal),
            ret => ret,
        }
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        self.0.finalize().and_then(|_| self.1.finalize())
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
