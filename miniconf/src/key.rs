use core::{iter::Fuse, num::NonZero};

use crate::Traversal;

/// Data to look up field names and convert to indices
///
/// This struct used together with [`crate::TreeKey`].
pub struct KeyLookup {
    /// The number of top-level nodes.
    ///
    /// This is used by `impl Keys for Packed`.
    pub len: NonZero<usize>,

    /// Node names, if any.
    ///
    /// If nodes have names, this is a slice of them.
    /// If it is `Some`, it's `.len()` is guaranteed to be `LEN`.
    pub names: Option<&'static [&'static str]>,
}

impl KeyLookup {
    /// Return a homogenenous unnamed KeyLookup
    #[inline]
    pub const fn homogeneous(len: usize) -> Self {
        match NonZero::new(len) {
            Some(len) => Self { len, names: None },
            None => panic!("Internal nodes must have at least one leaf"),
        }
    }

    /// Perform a index-to-name lookup
    pub fn lookup(&self, index: usize) -> Result<Option<&'static str>, Traversal> {
        match self.names {
            Some(names) => {
                debug_assert!(names.len() == self.len.get());
                match names.get(index) {
                    Some(name) => Ok(Some(name)),
                    None => Err(Traversal::NotFound(1)),
                }
            }
            None => {
                if index >= self.len.get() {
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
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        T::find(self, lookup)
    }
}

impl<T: Key> Key for &mut T
where
    T: Key + ?Sized,
{
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        T::find(self, lookup)
    }
}

// index
macro_rules! impl_key_integer {
    ($($t:ty)+) => {$(
        impl Key for $t {
            fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
                let index = (*self).try_into().or(Err(Traversal::NotFound(1)))?;
                if index >= lookup.len.get() {
                    Err(Traversal::NotFound(1))
                } else {
                    Ok(index)
                }
            }
        }
    )+};
}
impl_key_integer!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

// name
impl Key for str {
    fn find(&self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        let index = match lookup.names {
            Some(names) => names.iter().position(|n| *n == self),
            None => self.parse().ok(),
        }
        .ok_or(Traversal::NotFound(1))?;
        if index >= lookup.len.get() {
            Err(Traversal::NotFound(1))
        } else {
            Ok(index)
        }
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`KeyLookup`] and convert to `usize` index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal>;

    /// Finalize the keys, ensure there are no more.
    fn finalize(&mut self) -> Result<(), Traversal>;

    /// Chain another `Keys` to this one.
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
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        T::next(self, lookup)
    }

    fn finalize(&mut self) -> Result<(), Traversal> {
        T::finalize(self)
    }
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
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        self.0.next().ok_or(Traversal::TooShort(0))?.find(lookup)
    }

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
    /// The specific [`Keys`] implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a [`Keys`] implementor.
    fn into_keys(self) -> Self::IntoKeys;
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

/// Concatenate two `Keys` of different types
pub struct Chain<T, U>(T, U);

impl<T, U> Chain<T, U> {
    /// Return a new concatenated `Keys`
    pub fn new(t: T, u: U) -> Self {
        Self(t, u)
    }
}

impl<T: Keys, U: Keys> Keys for Chain<T, U> {
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        match self.0.next(lookup) {
            Err(Traversal::TooShort(_)) => self.1.next(lookup),
            ret => ret,
        }
    }

    fn finalize(&mut self) -> Result<(), Traversal> {
        self.0.finalize().and_then(|()| self.1.finalize())
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
