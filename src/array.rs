use super::{Error, IterError, Metadata, Miniconf};
use core::{
    fmt::Write,
    ops::{Deref, DerefMut},
};

/// An array that exposes each element through their [`Miniconf`] implementation.
///
/// # Design
///
/// Miniconf supports homogeneous arrays of items contained in structures using two forms. For the
/// [`miniconf::Array`](Array), each item of the array is accessed as a [`Miniconf`] tree.
///
/// For standard arrays of [`[T; N]`](array) form, by default the entire array is accessed as one atomic
/// value. By adding the `#[miniconf(defer)]` attribute, each index of the array is is instead accessed as
/// one atomic value (i.e. a single Miniconf item).
///
/// The type you should use depends on what data is contained in your array. If your array contains
/// `Miniconf` items, you can (and often want to) use [`Array`] and the `#[miniconf(defer)]` attribute.
/// However, if each element in your list is individually configurable as a single value (e.g. a list
/// of `u32`), then you must use a standard [`[T; N]`](array) array but you may optionally
/// `#[miniconf(defer)]` access to individual indices.
///
/// # Construction
///
/// An `Array` can be constructed using [`From<[T; N]>`](From)/[`Into<miniconf::Array>`]
/// and the contained value can be accessed through [`Deref`]/[`DerefMut`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Array<T, const N: usize>([T; N]);

impl<T, const N: usize> Deref for Array<T, N> {
    type Target = [T; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const N: usize> DerefMut for Array<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Default + Copy, const N: usize> Default for Array<T, N> {
    fn default() -> Self {
        Self([T::default(); N])
    }
}

impl<T, const N: usize> From<[T; N]> for Array<T, N> {
    fn from(x: [T; N]) -> Self {
        Self(x)
    }
}

impl<T, const N: usize> AsRef<[T; N]> for Array<T, N> {
    fn as_ref(&self) -> &[T; N] {
        self
    }
}

impl<T, const N: usize> AsMut<[T; N]> for Array<T, N> {
    fn as_mut(&mut self) -> &mut [T; N] {
        self
    }
}

impl<T, const N: usize> IntoIterator for Array<T, N> {
    type Item = T;
    type IntoIter = <[T; N] as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a Array<T, N> {
    type Item = <&'a [T; N] as IntoIterator>::Item;
    type IntoIter = <&'a [T; N] as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut Array<T, N> {
    type Item = <&'a mut [T; N] as IntoIterator>::Item;
    type IntoIter = <&'a mut [T; N] as IntoIterator>::IntoIter;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T, const N: usize> From<Array<T, N>> for [T; N] {
    fn from(x: Array<T, N>) -> Self {
        x.0
    }
}

/// Returns the number of digits required to format an integer less than `x`.
const fn digits(x: usize) -> usize {
    let mut n = 10;
    let mut num_digits = 1;

    while x > n {
        n *= 10;
        num_digits += 1;
    }
    num_digits
}

impl<T: Miniconf, const N: usize> Miniconf for Array<T, N> {
    fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        let i = self.0.index(path_parts.next())?;

        self.0
            .get_mut(i)
            .ok_or(Error::BadIndex)?
            .set_path(path_parts, de)
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        let i = self.0.index(path_parts.next())?;

        self.0
            .get(i)
            .ok_or(Error::BadIndex)?
            .get_path(path_parts, ser)
    }

    fn metadata(separator_length: usize) -> Metadata {
        let mut meta = T::metadata(separator_length);

        // We add separator and index
        meta.max_length += separator_length + digits(N);
        meta.max_depth += 1;
        meta.count *= N;

        meta
    }

    fn next_path(
        state: &[usize],
        depth: usize,
        mut topic: impl Write,
        separator: char,
    ) -> Result<usize, IterError> {
        match state.get(depth) {
            Some(&i) if i < N => {
                topic
                    .write_char(separator)
                    .and_then(|_| topic.write_str(itoa::Buffer::new().format(i)))
                    .map_err(|_| IterError::Length)?;
                T::next_path(state, depth + 1, topic, separator)
            }
            Some(_) => Err(IterError::Next(depth)),
            None => Err(IterError::Depth),
        }
    }
}

trait IndexLookup {
    fn index(&self, next: Option<&str>) -> Result<usize, Error>;
}

impl<T, const N: usize> IndexLookup for [T; N] {
    fn index(&self, next: Option<&str>) -> Result<usize, Error> {
        let next = next.ok_or(Error::PathTooShort)?;

        // Parse what should be the index value
        next.parse().map_err(|_| Error::BadIndex)
    }
}

impl<T: crate::Serialize + crate::DeserializeOwned, const N: usize> Miniconf for [T; N] {
    fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        let i = self.index(path_parts.next())?;

        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        let item = <[T]>::get_mut(self, i).ok_or(Error::BadIndex)?;
        *item = serde::Deserialize::deserialize(de).map_err(|_| Error::Deserialization)?;
        Ok(())
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        let i = self.index(path_parts.next())?;

        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        let item = <[T]>::get(self, i).ok_or(Error::BadIndex)?;
        serde::Serialize::serialize(item, ser).map_err(|_| Error::Serialization)
    }

    fn metadata(separator_length: usize) -> Metadata {
        Metadata {
            // We add separator and index
            max_length: separator_length + digits(N),
            max_depth: 1,
            count: N,
        }
    }

    fn next_path(
        state: &[usize],
        depth: usize,
        mut path: impl Write,
        separator: char,
    ) -> Result<usize, IterError> {
        match state.get(depth) {
            Some(&i) if i < N => {
                path.write_char(separator)
                    .and_then(|_| path.write_str(itoa::Buffer::new().format(i)))
                    .map_err(|_| IterError::Length)?;
                Ok(depth)
            }
            Some(_) => Err(IterError::Next(depth)),
            None => Err(IterError::Depth),
        }
    }
}
