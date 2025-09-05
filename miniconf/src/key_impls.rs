use core::{
    fmt::Write,
    ops::{Deref, DerefMut},
};

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{DescendError, Internal, IntoKeys, Key, Schema, Track, Transcode};

// index
macro_rules! impl_key_integer {
    ($($t:ty)+) => {$(
        impl Key for $t {
            #[inline]
            fn find(&self, internal: &Internal) -> Option<usize> {
                (*self).try_into().ok().filter(|i| *i < internal.len().get())
            }
        }
    )+};
}
impl_key_integer!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

/// Indices of `usize` to identify a node in a `TreeSchema`
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Indices<T: ?Sized> {
    len: usize,
    data: T,
}

impl<T> Indices<T> {
    /// Create a new `Indices`
    pub fn new(data: T, len: usize) -> Self {
        Self { len, data }
    }

    /// The length of the indices keys
    pub fn len(&self) -> usize {
        self.len
    }

    /// See [`Self::len()`]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Split indices into data and length
    pub fn into_inner(self) -> (T, usize) {
        (self.data, self.len)
    }
}

impl<T> From<T> for Indices<T> {
    #[inline]
    fn from(value: T) -> Self {
        Self {
            len: 0,
            data: value,
        }
    }
}

impl<U, T: AsRef<[U]> + ?Sized> AsRef<[U]> for Indices<T> {
    #[inline]
    fn as_ref(&self) -> &[U] {
        &self.data.as_ref()[..self.len]
    }
}

impl<'a, U, T: ?Sized> IntoIterator for &'a Indices<T>
where
    &'a T: IntoIterator<Item = U>,
{
    type Item = U;

    type IntoIter = core::iter::Take<<&'a T as IntoIterator>::IntoIter>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        (&self.data).into_iter().take(self.len)
    }
}

impl<T: AsMut<[usize]> + ?Sized> Transcode for Indices<T> {
    type Error = <[usize] as Transcode>::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        let mut slic = Track::new(self.data.as_mut());
        let ret = slic.transcode(schema, keys);
        self.len = slic.depth;
        ret
    }
}

macro_rules! impl_transcode_slice {
    ($($t:ty)+) => {$(
        impl Transcode for [$t] {
            type Error = ();

            fn transcode(&mut self, schema: &Schema, keys: impl IntoKeys) -> Result<(), DescendError<Self::Error>> {
                let mut it = self.iter_mut();
                schema.descend(keys.into_keys(), |_meta, idx_schema| {
                    if let Some((index, internal)) = idx_schema {
                        debug_assert!(internal.len().get() <= <$t>::MAX as _);
                        let i = index.try_into().or(Err(()))?;
                        let idx = it.next().ok_or(())?;
                        *idx = i;
                    }
                    Ok(())
                })
            }
        }
    )+};
}
impl_transcode_slice!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

#[cfg(feature = "alloc")]
impl<T> Transcode for Vec<T>
where
    usize: TryInto<T>,
{
    type Error = <usize as TryInto<T>>::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_meta, idx_schema| {
            if let Some((index, _schema)) = idx_schema {
                self.push(index.try_into()?);
            }
            Ok(())
        })
    }
}

////////////////////////////////////////////////////////////////////

// name
impl Key for str {
    #[inline]
    fn find(&self, internal: &Internal) -> Option<usize> {
        internal.get_index(self)
    }
}

/// Path with named keys separated by a separator char
///
/// The path will either be empty or start with the separator.
///
/// * `path: T`: A `Write` to write the separators and node names into during `Transcode`.
///   See also [Schema::transcode()] and `Shape.max_length` for upper bounds
///   on path length. Can also be a `AsRef<str>` to implement `IntoKeys` (see [`crate::KeysIter`]).
/// * `const S: char`: The path hierarchy separator to be inserted before each name,
///   e.g. `'/'`.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Path<T: ?Sized, const S: char>(pub T);

impl<T: ?Sized, const S: char> Path<T, S> {
    /// The path hierarchy separator
    #[inline]
    pub const fn separator(&self) -> char {
        S
    }
}

impl<T, const S: char> Path<T, S> {
    /// Extract just the path
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: ?Sized, const S: char> Deref for Path<T, S> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized, const S: char> DerefMut for Path<T, S> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: core::fmt::Display, const S: char> core::fmt::Display for Path<T, S> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

/// String split/skip wrapper, smaller/simpler than `.split(S).skip(1)`
#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct PathIter<'a, const S: char>(Option<&'a str>);

impl<'a, const S: char> PathIter<'a, S> {
    /// Create a new `PathIter`
    #[inline]
    pub fn new(s: Option<&'a str>) -> Self {
        Self(s)
    }

    /// Create a new `PathIter` starting at the root.
    ///
    /// This calls `next()` once to pop everything up to and including the first separator.
    #[inline]
    pub fn root(s: &'a str) -> Self {
        let mut s = Self(Some(s));
        // Skip the first part to disambiguate between
        // the one-Key Keys `[""]` and the zero-Key Keys `[]`.
        // This is relevant in the case of e.g. `Option` and newtypes.
        // See the corresponding unittests (`just_option`).
        // It implies that Paths start with the separator
        // or are empty. Everything before the first separator is ignored.
        // This also means that paths can always be concatenated without having to
        // worry about adding/trimming leading or trailing separators.
        s.next();
        s
    }
}

impl<'a, const S: char> Iterator for PathIter<'a, S> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.map(|s| {
            let pos = s
                .chars()
                .map_while(|c| (c != S).then_some(c.len_utf8()))
                .sum();
            let (left, right) = s.split_at(pos);
            self.0 = right.get(S.len_utf8()..);
            left
        })
    }
}

impl<const S: char> core::iter::FusedIterator for PathIter<'_, S> {}

impl<'a, T: AsRef<str> + ?Sized, const S: char> IntoKeys for Path<&'a T, S> {
    type IntoKeys = <PathIter<'a, S> as IntoKeys>::IntoKeys;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        PathIter::root(self.0.as_ref()).into_keys()
    }
}

impl<'a, T: AsRef<str> + ?Sized, const S: char> IntoKeys for &'a Path<T, S> {
    type IntoKeys = <Path<&'a str, S> as IntoKeys>::IntoKeys;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        Path(self.0.as_ref()).into_keys()
    }
}

impl<T: Write + ?Sized, const S: char> Transcode for Path<T, S> {
    type Error = core::fmt::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_meta, idx_schema| {
            if let Some((index, internal)) = idx_schema {
                self.0.write_char(S)?;
                let mut buf = itoa::Buffer::new();
                let name = internal
                    .get_name(index)
                    .unwrap_or_else(|| buf.format(index));
                debug_assert!(!name.contains(S));
                self.0.write_str(name)
            } else {
                Ok(())
            }
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn strsplit() {
        use heapless::Vec;
        for p in ["/d/1", "/a/bccc//d/e/", "", "/", "a/b", "a"] {
            let a: Vec<_, 10> = PathIter::<'_, '/'>::root(p).collect();
            let b: Vec<_, 10> = p.split('/').skip(1).collect();
            assert_eq!(a, b);
        }
    }
}
