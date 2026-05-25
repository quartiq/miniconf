use core::fmt::Write;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{DescendError, Internal, IntoKeys, Key, KeyError, Keys, Schema, Transcode};

// index
macro_rules! impl_key_integer {
    ($($t:ty)+) => {$(
        impl Key for $t {
            fn find(&self, internal: &Internal) -> Option<usize> {
                (*self).try_into().ok().filter(|i| *i < internal.len().get())
            }
        }
    )+};
}
impl_key_integer!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

/// Sequence of child indices identifying a node in a `TreeSchema`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Indices<T: ?Sized> {
    len: usize,
    data: T,
}

impl<T> Indices<T> {
    /// Create a new `Indices`
    pub fn new(data: T, len: usize) -> Self {
        Self { len, data }
    }

    /// Split indices into data and length
    pub fn into_inner(self) -> (T, usize) {
        (self.data, self.len)
    }
}

impl<T: ?Sized> Indices<T> {
    /// The number of selector segments in this index path.
    pub fn len(&self) -> usize {
        self.len
    }

    /// See [`Self::len()`]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<T> From<T> for Indices<T> {
    fn from(value: T) -> Self {
        Self {
            len: 0,
            data: value,
        }
    }
}

impl<U, T: AsRef<[U]> + ?Sized> AsRef<[U]> for Indices<T> {
    fn as_ref(&self) -> &[U] {
        &self.data.as_ref()[..self.len]
    }
}

impl<U, T: AsMut<[U]> + ?Sized> AsMut<[U]> for Indices<T> {
    fn as_mut(&mut self) -> &mut [U] {
        &mut self.data.as_mut()[..self.len]
    }
}

impl<'a, U, T: ?Sized> IntoIterator for &'a Indices<T>
where
    &'a T: IntoIterator<Item = U>,
{
    type Item = U;

    type IntoIter = core::iter::Take<<&'a T as IntoIterator>::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
        (&self.data).into_iter().take(self.len)
    }
}

impl<T: AsMut<[usize]> + ?Sized> Transcode for Indices<T> {
    type Error = <[usize] as Transcode>::Error;

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        self.len = 0;
        schema.descend(keys, |_meta, idx_schema| {
            if let Some((index, _schema)) = idx_schema {
                let idx = self.data.as_mut().get_mut(self.len).ok_or(())?;
                *idx = index;
                self.len += 1;
            }
            Ok(())
        })
    }
}

macro_rules! impl_transcode_slice {
    ($($t:ty)+) => {$(
        impl Transcode for [$t] {
            type Error = ();

            fn transcode_from(&mut self, schema: &Schema, keys: impl Keys) -> Result<(), DescendError<Self::Error>> {
                let mut it = self.iter_mut();
                schema.descend(keys, |_meta, idx_schema| {
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

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys, |_meta, idx_schema| {
            if let Some((index, _schema)) = idx_schema {
                self.push(index.try_into()?);
            }
            Ok(())
        })
    }
}

#[cfg(feature = "heapless")]
impl<T, const N: usize> Transcode for heapless::Vec<T, N>
where
    usize: TryInto<T>,
{
    type Error = ();

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys, |_meta, idx_schema| {
            if let Some((index, _schema)) = idx_schema {
                let i = index.try_into().or(Err(()))?;
                self.push(i).or(Err(()))?;
            }
            Ok(())
        })
    }
}

#[cfg(feature = "heapless-09")]
impl<T, const N: usize> Transcode for heapless_09::Vec<T, N>
where
    usize: TryInto<T>,
{
    type Error = ();

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys, |_meta, idx_schema| {
            if let Some((index, _schema)) = idx_schema {
                let i = index.try_into().or(Err(()))?;
                self.push(i).or(Err(()))?;
            }
            Ok(())
        })
    }
}

////////////////////////////////////////////////////////////////////

// name
impl Key for str {
    fn find(&self, internal: &Internal) -> Option<usize> {
        internal.get_index(self)
    }
}

/// Output path with named selector segments separated by a separator char.
///
/// The path will either be empty or start with the separator.
///
/// * `path: T`: A `Write` to write the separators and node names into during `Transcode`.
///   See also [`Schema::transcode()`] and `Shape.max_length` for upper bounds
///   on path length. Use [`PathIter`] for boundary key input.
/// * `separator`: The path hierarchy separator to be inserted before each name,
///   e.g. `'/'`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Path<T> {
    /// The underlying path buffer or string.
    pub path: T,
    /// The path hierarchy separator.
    pub separator: char,
}

impl<T> Default for Path<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            path: T::default(),
            separator: '/',
        }
    }
}

impl<T> Path<T> {
    /// Create a new `Path`.
    pub const fn new(path: T, separator: char) -> Self {
        Self { path, separator }
    }

    /// The path hierarchy separator
    pub const fn separator(&self) -> char {
        self.separator
    }

    /// Extract just the path
    pub fn into_inner(self) -> T {
        self.path
    }
}

impl<T: AsRef<str>> AsRef<str> for Path<T> {
    fn as_ref(&self) -> &str {
        self.path.as_ref()
    }
}

impl<T: core::fmt::Display> core::fmt::Display for Path<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.path.fmt(f)
    }
}

/// Const-specialized output path with named selector segments separated by a const separator.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct ConstPath<T, const S: char>(pub T);

impl<T, const S: char> ConstPath<T, S> {
    /// The path hierarchy separator.
    pub const fn separator(&self) -> char {
        S
    }

    /// Extract just the path.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: AsRef<str>, const S: char> AsRef<str> for ConstPath<T, S> {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl<T: core::fmt::Display, const S: char> core::fmt::Display for ConstPath<T, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

fn split_path(path: &str, separator: char) -> (&str, Option<&str>) {
    let pos = path
        .chars()
        .map_while(|c| (c != separator).then_some(c.len_utf8()))
        .sum();
    let (left, right) = path.split_at(pos);
    (left, right.get(separator.len_utf8()..))
}

/// Runtime-separated path iterator for boundary key input.
#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct PathIter<'a> {
    path: Option<&'a str>,
    separator: char,
}

impl<'a> PathIter<'a> {
    /// Create a new `PathIter`.
    pub fn new(path: Option<&'a str>, separator: char) -> Self {
        Self { path, separator }
    }

    /// Create a new `PathIter` starting at the root.
    ///
    /// This calls `next()` once to pop everything up to and including the first separator.
    pub fn root(path: &'a str, separator: char) -> Self {
        let mut s = Self::new(Some(path), separator);
        // Skip the first part to disambiguate between
        // the one-Key Keys `[""]` and the zero-Key Keys `[]`.
        // This is relevant in the case of e.g. `Option` and newtypes.
        // See the corresponding unittests (`just_option`).
        // It implies that Paths start with the separator
        // or are empty. Everything before the first separator is ignored.
        // This also means that paths can always be concatenated without having to
        // worry about adding/trimming leading or trailing separators.
        Iterator::next(&mut s);
        s
    }
}

impl<'a> Iterator for PathIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let (left, right) = split_path(self.path?, self.separator);
        self.path = right;
        Some(left)
    }
}

impl core::iter::FusedIterator for PathIter<'_> {}

impl Keys for PathIter<'_> {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let key = Iterator::next(self).ok_or(KeyError::TooShort)?;
        <str as Key>::find(key, internal).ok_or(KeyError::NotFound)
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        match Iterator::next(self) {
            Some(_) => Err(KeyError::TooLong),
            None => Ok(()),
        }
    }
}

/// Const-specialized path iterator for boundary key input.
#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConstPathIter<'a, const S: char>(Option<&'a str>);

impl<'a, const S: char> ConstPathIter<'a, S> {
    /// Create a new const-specialized `PathIter`.
    pub fn new(path: Option<&'a str>) -> Self {
        Self(path)
    }

    /// Create a new const-specialized `PathIter` starting at the root.
    pub fn root(path: &'a str) -> Self {
        let mut s = Self::new(Some(path));
        Iterator::next(&mut s);
        s
    }
}

impl<'a, const S: char> Iterator for ConstPathIter<'a, S> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let s = self.0?;
        if S.is_ascii() {
            if let Some(i) = s.as_bytes().iter().position(|b| *b == S as u8) {
                self.0 = s.get(i + 1..);
                s.get(..i)
            } else {
                self.0 = None;
                Some(s)
            }
        } else {
            let (left, right) = split_path(s, S);
            self.0 = right;
            Some(left)
        }
    }
}

impl<const S: char> core::iter::FusedIterator for ConstPathIter<'_, S> {}

impl<const S: char> Keys for ConstPathIter<'_, S> {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let key = Iterator::next(self).ok_or(KeyError::TooShort)?;
        <str as Key>::find(key, internal).ok_or(KeyError::NotFound)
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        match Iterator::next(self) {
            Some(_) => Err(KeyError::TooLong),
            None => Ok(()),
        }
    }
}

impl<'a> IntoKeys for PathIter<'a> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

impl<'a, const S: char> IntoKeys for ConstPathIter<'a, S> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

impl<T: Write> Transcode for Path<T> {
    type Error = core::fmt::Error;

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys, |_meta, idx_schema| {
            if let Some((index, internal)) = idx_schema {
                self.path.write_char(self.separator)?;
                let mut buf = itoa::Buffer::new();
                let name = internal
                    .get_name(index)
                    .unwrap_or_else(|| buf.format(index));
                debug_assert!(!name.contains(self.separator));
                self.path.write_str(name)
            } else {
                Ok(())
            }
        })
    }
}

impl<T: Write, const S: char> Transcode for ConstPath<T, S> {
    type Error = core::fmt::Error;

    fn transcode_from(
        &mut self,
        schema: &Schema,
        keys: impl Keys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys, |_meta, idx_schema| {
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
        use heapless_09::Vec;
        for p in ["/d/1", "/a/bccc//d/e/", "", "/", "a/b", "a"] {
            let a: Vec<_, 10> = PathIter::root(p, '/').collect();
            let b: Vec<_, 10> = p.split('/').skip(1).collect();
            assert_eq!(a, b);
        }
    }

    #[test]
    fn ascii_strsplit() {
        use heapless_09::Vec;
        for p in ["/d/1", "/a/bccc//d/e/", "", "/", "a/b", "a"] {
            let a: Vec<_, 10> = ConstPathIter::<'/'>::root(p).collect();
            let b: Vec<_, 10> = p.split('/').skip(1).collect();
            assert_eq!(a, b);
        }
    }
}
