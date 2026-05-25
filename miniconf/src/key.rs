//! Key naming rules:
//!
//! - [`Key`] is one selector segment: one child name or one child index.
//! - [`Keys`] is a normalized cursor over zero or more selector segments.
//! - [`IntoKeys`] is the outer boundary funnel into a normalized [`Keys`] cursor.
//! - Public wrapper APIs named `*_by_key` accept `impl IntoKeys`.
//! - Core APIs named `*_by_keys` operate on normalized `impl Keys`.

use core::{convert::Infallible, iter::Fuse};

use crate::{ConstPathIter, DescendError, Internal, KeyError, Schema};

/// Convert one selector segment into a child index for an internal node schema.
pub trait Key {
    /// Resolve this selector segment to a child index.
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

/// Normalized cursor over selector segments.
pub trait Keys {
    /// Resolve the next selector segment in `internal` to a child index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError>;

    /// Finalize the cursor and ensure there are no more selector segments.
    ///
    /// This must be fused.
    fn finalize(&mut self) -> Result<(), KeyError>;
}

impl<T: Keys + ?Sized> Keys for &mut T {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        (**self).next(internal)
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        (**self).finalize()
    }
}

impl<T: Key> Keys for &[T] {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let (key, tail) = self.split_first().ok_or(KeyError::TooShort)?;
        let index = key.find(internal).ok_or(KeyError::NotFound)?;
        *self = tail;
        Ok(index)
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(KeyError::TooLong)
        }
    }
}

/// Boundary input that can be normalized into a [`Keys`] cursor.
pub trait IntoKeys {
    /// The specific `Keys` implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a normalized [`Keys`] implementor.
    ///
    /// This is the outer boundary funnel. Accept wider ergonomic key inputs here, but keep the
    /// actual `Keys` type space small so deep traversal APIs (`*_by_keys()`, schema descent, and
    /// transcoding) do not monomorphize over every input wrapper/container flavor.
    fn into_keys(self) -> Self::IntoKeys;

    /// Concatenate two boundary key inputs into one normalized key stream.
    ///
    /// This lives on [`IntoKeys`], not [`Keys`], because chaining is boundary composition rather
    /// than a concern of deep traversal APIs.
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self::IntoKeys, U::IntoKeys>
    where
        Self: Sized,
    {
        Chain(self.into_keys(), other.into_keys())
    }
}

/// Look up a key path in a [`Schema`] and transcode it.
pub trait Transcode {
    /// The possible error when transcoding.
    ///
    /// Use this to indicate no space or unencodable/invalid values
    type Error;

    /// Perform a node lookup from a boundary key input and transcode it.
    ///
    /// This is the low-level, in-place transcoding API. Fresh output construction is provided by
    /// [`Transcode::transcode()`]. Existing target content handling is representation-specific:
    /// fixed-capacity/key views typically overwrite, while append-oriented buffers and writers may
    /// append.
    ///
    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>>;

    /// Transcode a boundary key input into a fresh default-constructed output.
    fn transcode(schema: &Schema, keys: impl IntoKeys) -> Result<Self, DescendError<Self::Error>>
    where
        Self: Sized + Default,
    {
        let mut target = Self::default();
        target.transcode_from(schema, keys.into_keys())?;
        Ok(target)
    }
}

impl<T: Transcode + ?Sized> Transcode for &mut T {
    type Error = T::Error;
    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
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
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys, |_, _| Ok::<_, Infallible>(()))
    }
}

/// Explicit normalized wrapper for iterator-shaped key inputs.
///
/// This exists so iterator inputs stay opt-in at the [`IntoKeys`] boundary instead of being
/// accepted by a blanket impl.
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct KeysIter<T>(Fuse<T>);

impl<T: Iterator> KeysIter<T> {
    pub(crate) fn new(inner: T) -> Self {
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

impl<'a> IntoKeys for &'a str {
    type IntoKeys = ConstPathIter<'a, '/'>;

    /// Interpret `self` as a rooted slash-separated path.
    ///
    /// Use [`crate::PathIter`] or [`crate::ConstPathIter`] directly for non-`'/'` separators.
    fn into_keys(self) -> Self::IntoKeys {
        ConstPathIter::root(self)
    }
}

impl<T: Key> IntoKeys for &[T] {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

impl<'a, T: Key, const N: usize> IntoKeys for &'a [T; N] {
    type IntoKeys = &'a [T];

    fn into_keys(self) -> Self::IntoKeys {
        &self[..]
    }
}

impl<T: Key, const N: usize> IntoKeys for [T; N] {
    type IntoKeys = KeysIter<core::array::IntoIter<T, N>>;

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

/// Concatenate two normalized key cursors.
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
