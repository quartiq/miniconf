use core::{convert::Infallible, iter::Fuse};

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
    /// [`FromConfig::transcode`] and [`FromConfig::transcode_with`]. Existing target content
    /// handling is representation-specific: fixed-capacity/key views typically overwrite, while
    /// append-oriented buffers and writers may append.
    ///
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

    /// Construct a fresh transcoding target from the provided configuration.
    fn from_config(config: &Self::Config) -> Self;

    /// Transcode keys into a fresh output constructed from `config`.
    fn transcode_with(
        schema: &Schema,
        keys: impl IntoKeys,
        config: Self::Config,
    ) -> Result<Self, DescendError<<Self as Transcode>::Error>>
    where
        Self: Transcode,
    {
        let mut target = Self::from_config(&config);
        target.transcode_from(schema, keys)?;
        Ok(target)
    }

    /// Transcode keys into a fresh output constructed from the default configuration.
    fn transcode(
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<Self, DescendError<<Self as Transcode>::Error>>
    where
        Self: Transcode,
    {
        Self::transcode_with(schema, keys, Self::DEFAULT_CONFIG)
    }
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

/// Shim to provide the bare lookup without transcoding target
impl Transcode for () {
    type Error = Infallible;
    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_, _| Ok::<_, Infallible>(()))
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
