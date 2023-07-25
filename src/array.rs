use super::{Error, Inner, IterError, Metadata, Miniconf, Outer};
use core::fmt::Write;

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

// Local type alias to minimize the rename drama for now: FIXME
pub type Array<T, const N: usize> = [T; N];

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

impl<T: Miniconf<Outer>, const N: usize> Miniconf<Inner> for [T; N] {
    fn set_path<'a, 'b: 'a, P, D>(
        &mut self,
        path_parts: &mut P,
        de: D,
    ) -> Result<(), Error<D::Error>>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        let next = path_parts.next().ok_or(Error::PathTooShort)?;
        let index: usize = next.parse().map_err(|_| Error::BadIndex)?;

        self.get_mut(index)
            .ok_or(Error::BadIndex)?
            .set_path(path_parts, de)
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        let next = path_parts.next().ok_or(Error::PathTooShort)?;
        let index: usize = next.parse().map_err(|_| Error::BadIndex)?;

        self.get(index)
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

impl<T: crate::Serialize + crate::DeserializeOwned, const N: usize> Miniconf<Outer> for [T; N] {
    fn set_path<'a, 'b: 'a, P, D>(
        &mut self,
        path_parts: &mut P,
        de: D,
    ) -> Result<(), Error<D::Error>>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        let next = path_parts.next().ok_or(Error::PathTooShort)?;
        let index: usize = next.parse().map_err(|_| Error::BadIndex)?;

        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        let item = <[T]>::get_mut(self, index).ok_or(Error::BadIndex)?;
        *item = serde::Deserialize::deserialize(de)?;
        Ok(())
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        let next = path_parts.next().ok_or(Error::PathTooShort)?;
        let index: usize = next.parse().map_err(|_| Error::BadIndex)?;

        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        let item = <[T]>::get(self, index).ok_or(Error::BadIndex)?;
        Ok(serde::Serialize::serialize(item, ser)?)
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
