use core::{iter::Fuse, num::NonZero};

use serde::Serialize;

use crate::Traversal;

/// Data to look up field names and convert to indices
///
/// This struct used together with [`crate::TreeKey`].
#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub enum KeyLookup {
    /// Named children
    Named(&'static [&'static str]),
    /// Numbered heterogeneous children
    Numbered(NonZero<usize>),
    /// Homogeneous numbered children
    Homogeneous(NonZero<usize>),
}

impl KeyLookup {
    /// Return a named KeyLookup
    #[inline]
    pub const fn named(names: &'static [&'static str]) -> Self {
        if names.is_empty() {
            panic!("Must have at least one child");
        }
        Self::Named(names)
    }

    /// Return a homogenenous, unnamed KeyLookup
    #[inline]
    pub const fn homogeneous(len: usize) -> Self {
        match NonZero::new(len) {
            Some(len) => Self::Homogeneous(len),
            None => panic!("Must have at least one child"),
        }
    }

    /// Return a heterogeneous numbered KeyLookup
    #[inline]
    pub const fn numbered(len: usize) -> Self {
        match NonZero::new(len) {
            Some(len) => Self::Numbered(len),
            None => panic!("Must have at least one child"),
        }
    }

    /// Return the number of elements in the lookup
    #[inline]
    pub const fn len(&self) -> NonZero<usize> {
        match self {
            Self::Named(names) => match NonZero::new(names.len()) {
                Some(len) => len,
                None => panic!("Must have at least one child"),
            },
            Self::Numbered(len) | Self::Homogeneous(len) => *len,
        }
    }

    /// Perform a index-to-name lookup
    #[inline]
    pub fn lookup(&self, index: usize) -> Result<Option<&'static str>, Traversal> {
        match self {
            Self::Named(names) => match names.get(index) {
                Some(name) => Ok(Some(name)),
                None => Err(Traversal::NotFound(1)),
            },
            Self::Numbered(len) | Self::Homogeneous(len) => {
                if index >= len.get() {
                    Err(Traversal::NotFound(1))
                } else {
                    Ok(None)
                }
            }
        }
    }
}

/// Convert a `&str` key into a node index on a `KeyLookup`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal>;
}

impl<T: Key> Key for &T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        (**self).find(lookup)
    }
}

impl<T: Key> Key for &mut T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        (**self).find(lookup)
    }
}

// index
macro_rules! impl_key_integer {
    ($($t:ty)+) => {$(
        impl Key for $t {
            #[inline]
            fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
                (*self)
                    .try_into()
                    .ok()
                    .filter(|i| *i < lookup.len().get())
                    .ok_or(Traversal::NotFound(1))
            }
        }
    )+};
}
impl_key_integer!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

// name
impl Key for str {
    #[inline]
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        match lookup {
            KeyLookup::Named(names) => names.iter().position(|n| *n == self),
            KeyLookup::Homogeneous(len) | KeyLookup::Numbered(len) => {
                self.parse().ok().filter(|i| *i < len.get())
            }
        }
        .ok_or(Traversal::NotFound(1))
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`KeyLookup`] and convert to `usize` index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal>;

    /// Finalize the keys, ensure there are no more.
    ///
    /// This must be fused.
    fn finalize(&mut self) -> Result<(), Traversal>;

    /// Chain another `Keys` to this one.
    #[inline]
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self, U::IntoKeys>
    where
        Self: Sized,
    {
        Chain(self, other.into_keys())
    }
}

impl<T> Keys for &mut T
where
    T: Keys + ?Sized,
{
    #[inline]
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        (**self).next(lookup)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        (**self).finalize()
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
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        self.0.next().ok_or(Traversal::TooShort(0))?.find(lookup)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        self.0
            .next()
            .is_none()
            .then_some(())
            .ok_or(Traversal::TooLong(0))
    }
}

/// Be converted into a `Keys`
pub trait IntoKeys {
    /// The specific `Keys` implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a `Keys` implementor.
    fn into_keys(self) -> Self::IntoKeys;
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

impl<T, U> Chain<T, U> {
    /// Return a new concatenated `Keys`
    #[inline]
    pub fn new(t: T, u: U) -> Self {
        Self(t, u)
    }
}

impl<T: Keys, U: Keys> Keys for Chain<T, U> {
    #[inline]
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        match self.0.next(lookup) {
            Err(Traversal::TooShort(_)) => self.1.next(lookup),
            ret => ret,
        }
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        self.0.finalize().and_then(|()| self.1.finalize())
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
