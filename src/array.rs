use crate::{Error, Increment, Key, Metadata, Miniconf, Ok, Result};
use core::ops::{Deref, DerefMut};

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
    fn name_to_index(value: &str) -> core::option::Option<usize> {
        value.parse().ok()
    }

    fn set_by_key<'a, P, D>(&mut self, keys: &mut P, de: D) -> Result<D::Error>
    where
        P: Iterator,
        D: serde::Deserializer<'a>,
        P::Item: Key,
    {
        let key = keys.next().ok_or(Error::Internal(0))?;
        let index = key.find::<Self>().ok_or(Error::NotFound(1))?;
        let item = self.0.get_mut(index).ok_or(Error::NotFound(1))?;
        item.set_by_key(keys, de).increment()
    }

    fn get_by_key<P, S>(&self, keys: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator,
        S: serde::Serializer,
        P::Item: Key,
    {
        let key = keys.next().ok_or(Error::Internal(0))?;
        let index = key.find::<Self>().ok_or(Error::NotFound(1))?;
        let item = self.0.get(index).ok_or(Error::NotFound(1))?;
        item.get_by_key(keys, ser).increment()
    }

    fn metadata() -> Metadata {
        let mut meta = T::metadata();

        meta.max_length += digits(N);
        meta.max_depth += 1;
        meta.count *= N;

        meta
    }

    fn traverse_by_key<P, F, E>(keys: &mut P, mut func: F) -> Result<E>
    where
        P: Iterator,
        P::Item: Key,
        F: FnMut(Ok, usize, &str) -> core::result::Result<(), E>,
    {
        match keys.next() {
            None => Ok(Ok::Internal(0)),
            Some(key) => {
                let index: usize = key.find::<Self>().ok_or(Error::NotFound(1))?;
                if index < N {
                    func(Ok::Internal(1), index, itoa::Buffer::new().format(index))?;
                    T::traverse_by_key(keys, func).increment()
                } else {
                    Err(Error::NotFound(1))
                }
            }
        }
    }
}

impl<T: serde::Serialize + serde::de::DeserializeOwned, const N: usize> Miniconf for [T; N] {
    fn name_to_index(value: &str) -> core::option::Option<usize> {
        value.parse().ok()
    }

    fn set_by_key<'a, P, D>(&mut self, keys: &mut P, de: D) -> Result<D::Error>
    where
        P: Iterator,
        D: serde::Deserializer<'a>,
        P::Item: Key,
    {
        let key = keys.next().ok_or(Error::Internal(0))?;
        if keys.next().is_some() {
            return Err(Error::TooLong(1));
        }
        let index: usize = key.find::<Self>().ok_or(Error::NotFound(1))?;
        let item = self.get_mut(index).ok_or(Error::NotFound(1))?;
        *item = serde::Deserialize::deserialize(de)?;
        Ok(Ok::Leaf(1))
    }

    fn get_by_key<P, S>(&self, keys: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator,
        S: serde::Serializer,
        P::Item: Key,
    {
        let key = keys.next().ok_or(Error::Internal(0))?;
        if keys.next().is_some() {
            return Err(Error::TooLong(1));
        }
        let index = key.find::<Self>().ok_or(Error::NotFound(1))?;
        let item = self.get(index).ok_or(Error::NotFound(1))?;
        serde::Serialize::serialize(item, ser)?;
        Ok(Ok::Leaf(1))
    }

    fn traverse_by_key<P, F, E>(keys: &mut P, mut func: F) -> Result<E>
    where
        P: Iterator,
        P::Item: Key,
        F: FnMut(Ok, usize, &str) -> core::result::Result<(), E>,
    {
        match keys.next() {
            None => Ok(Ok::Internal(0)),
            Some(key) => {
                let index = key.find::<Self>().ok_or(Error::NotFound(1))?;
                if index < N {
                    func(Ok::Leaf(1), index, itoa::Buffer::new().format(index))
                        .map_err(|e| Error::Inner(e))?;
                    Ok(Ok::Leaf(1))
                } else {
                    Err(Error::NotFound(1))
                }
            }
        }
    }

    fn metadata() -> Metadata {
        Metadata {
            max_length: digits(N),
            max_depth: 1,
            count: N,
        }
    }
}
