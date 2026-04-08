use core::{convert::Infallible, iter::Fuse};

use serde::Serialize;

use crate::{DescendError, Internal, KeyError, Schema};

/// Convert a key into a node index given an internal node schema
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find(&self, internal: &Internal) -> Option<usize>;
}

impl<T: Key + ?Sized> Key for &T {
    fn find(&self, internal: &Internal) -> Option<usize> {
        (**self).find(internal)
    }
}

impl<T: Key + ?Sized> Key for &mut T {
    fn find(&self, internal: &Internal) -> Option<usize> {
        (**self).find(internal)
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`Internal`] and convert to `usize` index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError>;

    /// Finalize the keys, ensure there are no more.
    ///
    /// This must be fused.
    fn finalize(&mut self) -> Result<(), KeyError>;

    /// Chain another `Keys` to this one.
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self, U::IntoKeys>
    where
        Self: Sized,
    {
        Chain(self, other.into_keys())
    }

    /// Track depth
    fn track(self) -> Track<Self>
    where
        Self: Sized,
    {
        Track {
            inner: self,
            depth: 0,
        }
    }
}

impl<T: Keys + ?Sized> Keys for &mut T {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        (**self).next(internal)
    }

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

/// Look up an `IntoKeys` in a `Schema` and transcode it.
pub trait Transcode {
    /// The possible error when transcoding.
    ///
    /// Use this to indicate no space or unencodable/invalid values
    type Error;

    /// Perform a node lookup of a `K: IntoKeys` on a `Schema` and transcode it.
    ///
    /// This is the low-level, in-place transcoding API. Fresh output construction is provided by
    /// [`Schema::transcode()`](crate::Schema::transcode) and [`FromConfig`]. Existing target content
    /// handling is representation-specific: fixed-capacity/key views typically overwrite, while
    /// append-oriented buffers and writers may append.
    ///
    /// Use this to report insufficient capacity or unencodable values at the depth where they
    /// occur.
    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>>;
}

/// Construct a fresh transcoding target from compact configuration state.
pub trait FromConfig: Sized {
    /// The configuration required to construct `Self`.
    type Config: Copy;

    /// Default configuration for `Self`.
    const DEFAULT_CONFIG: Self::Config;

    /// Construct a fresh transcoding target from the provided seed.
    fn from_config(config: &Self::Config) -> Self;
}

impl<T: Transcode + ?Sized> Transcode for &mut T {
    type Error = T::Error;
    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        (**self).transcode_from(schema, keys)
    }
}

/// Track key depth
///
/// This tracks the depth during [`Keys`] and [`Transcode`].
#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Track<K> {
    /// The inner keys
    inner: K,
    /// The keys terminate at the given depth
    depth: usize,
}

impl<K> Track<K> {
    /// Create a new `Track`
    pub fn new(inner: K) -> Self {
        Self { inner, depth: 0 }
    }

    /// Whether a leaf node as been encountered
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Borrow the inner `Keys`
    pub fn inner(&self) -> &K {
        &self.inner
    }

    /// Split into inner `Keys` and leaf node flag
    pub fn into_inner(self) -> (K, usize) {
        (self.inner, self.depth)
    }
}

impl<K: Keys> IntoKeys for &mut Track<K> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self.depth = 0;
        self
    }
}

impl<K: Keys> Keys for Track<K> {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let k = self.inner.next(internal);
        if k.is_ok() {
            self.depth += 1;
        }
        k
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        self.inner.finalize()
    }
}

impl<T: Transcode> Transcode for Track<T> {
    type Error = T::Error;

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        self.depth = 0;
        let mut tracked = keys.into_keys().track();
        let ret = self.inner.transcode_from(schema, &mut tracked);
        self.depth = tracked.depth;
        ret
    }
}

impl<T: FromConfig> FromConfig for Track<T> {
    type Config = T::Config;
    const DEFAULT_CONFIG: Self::Config = T::DEFAULT_CONFIG;

    fn from_config(config: &Self::Config) -> Self {
        Self::new(T::from_config(config))
    }
}

/// Shim to provide the bare lookup/Track without transcoding target
impl Transcode for () {
    type Error = Infallible;
    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_, _| Ok(()))
    }
}

impl FromConfig for () {
    type Config = ();
    const DEFAULT_CONFIG: Self::Config = ();

    fn from_config(_: &Self::Config) -> Self {}
}

/// [`Keys`]/[`IntoKeys`] for Iterators of [`Key`]
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct KeysIter<T>(Fuse<T>);

impl<T: Iterator> KeysIter<T> {
    fn new(inner: T) -> Self {
        Self(inner.fuse())
    }
}

impl<T> Keys for KeysIter<T>
where
    T: Iterator,
    T::Item: Key,
{
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let n = self.0.next().ok_or(KeyError::TooShort)?;
        n.find(internal).ok_or(KeyError::NotFound)
    }

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

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

/// Concatenate two `Keys` of different types
pub struct Chain<T, U>(T, U);

impl<T: Keys, U: Keys> Keys for Chain<T, U> {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        match self.0.next(internal) {
            Err(KeyError::TooShort) => self.1.next(internal),
            ret => ret,
        }
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        self.0.finalize().and_then(|_| self.1.finalize())
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
