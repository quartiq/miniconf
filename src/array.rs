//! Array support
//!
//! # Design
//! Miniconf supports homogeneous arrays of items contained in structures using two forms. For the
//! [`Array`], each item of the array is accessed as a `Miniconf` tree.
//!
//! For standard arrays of [T; N] form, each item of the array is accessed as one atomic
//! value (i.e. a single Miniconf item).
//!
//! The type you should use depends on what data is contained in your array. If your array contains
//! `Miniconf` items, you can (and often want to) use [`Array`]. However, if each element in your list is
//! individually configurable as a single value (e.g. a list of u32), then you must use a
//! standard [T; N] array.
use super::{Error, Metadata, Miniconf, Peekable};

use core::fmt::Write;

/// An array that exposes each element through their [`Miniconf`](trait.Miniconf.html) implementation.
pub struct Array<T, const N: usize>(pub [T; N]);

impl<T, const N: usize> core::ops::Deref for Array<T, N> {
    type Target = [T; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T, const N: usize> core::ops::DerefMut for Array<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Default + Copy, const N: usize> Default for Array<T, N> {
    fn default() -> Self {
        Self([T::default(); N])
    }
}

impl<T: core::fmt::Debug, const N: usize> core::fmt::Debug for Array<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq, const N: usize> PartialEq<[T; N]> for Array<T, N> {
    fn eq(&self, other: &[T; N]) -> bool {
        self.0.eq(other)
    }
}

impl<T: PartialEq, const N: usize> PartialEq for Array<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Clone, const N: usize> Clone for Array<T, N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy, const N: usize> Copy for Array<T, N> {}

const fn digits(x: usize) -> usize {
    let mut n = 10;
    let mut num_digits = 1;

    while x >= n {
        n *= 10;
        num_digits += 1;
    }
    num_digits
}

impl<T: Miniconf, const N: usize> Miniconf for Array<T, N> {
    fn set_path<'a, P: Peekable<Item = &'a str>>(
        &mut self,
        path_parts: &'a mut P,
        value: &[u8],
    ) -> Result<(), Error> {
        let i = self.0.index(path_parts.next())?;

        self.0
            .get_mut(i)
            .ok_or(Error::BadIndex)?
            .set_path(path_parts, value)
    }

    fn get_path<'a, P: Peekable<Item = &'a str>>(
        &self,
        path_parts: &'a mut P,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        let i = self.0.index(path_parts.next())?;

        self.0
            .get(i)
            .ok_or(Error::BadIndex)?
            .get_path(path_parts, value)
    }

    fn metadata() -> Metadata {
        let mut meta = T::metadata();

        // Unconditionally account for separator since we add it
        // even if elements that are deferred to (`Options`)
        // may have no further hierarchy to add and remove the separator again.
        meta.max_length += digits(N - 1) + 1;
        meta.max_depth += 1;

        meta
    }

    fn next_path<const TS: usize>(
        &self,
        state: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> bool {
        // Note(unreachable): During expected execution paths using `into_iter()`, the size of the
        // index stack is checked in advance to make sure this condition doesn't occur.
        // However, it's possible to happen if the user manually calls `next_path`.
        if state.is_empty() {
            unreachable!("Index stack too small");
        }

        let original_length = topic.len();

        while state[0] < N {
            // Add the array index and separator to the topic name.
            if write!(topic, "{}/", state[0]).is_err() {
                unreachable!("Topic buffer too short");
            }

            if self.0[state[0]].next_path(&mut state[1..], topic) {
                return true;
            }

            // Strip off the previously prepended index, since we completed that element and need
            // to instead check the next one.
            topic.truncate(original_length);

            state[0] += 1;
            state[1..].iter_mut().for_each(|x| *x = 0);
        }

        false
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
    fn set_path<'a, P: Peekable<Item = &'a str>>(
        &mut self,
        path_parts: &mut P,
        value: &[u8],
    ) -> Result<(), Error> {
        let i = self.index(path_parts.next())?;

        if path_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        let item = <[T]>::get_mut(self, i).ok_or(Error::BadIndex)?;
        *item = serde_json_core::from_slice(value)?.0;
        Ok(())
    }

    fn get_path<'a, P: Peekable<Item = &'a str>>(
        &self,
        path_parts: &mut P,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        let i = self.index(path_parts.next())?;

        if path_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        let item = <[T]>::get(self, i).ok_or(Error::BadIndex)?;
        serde_json_core::to_slice(item, value).map_err(|_| Error::SerializationFailed)
    }

    fn metadata() -> Metadata {
        Metadata {
            max_length: digits(N - 1),
            max_depth: 1,
        }
    }

    fn next_path<const TS: usize>(
        &self,
        state: &mut [usize],
        path: &mut heapless::String<TS>,
    ) -> bool {
        // Note(unreachable): During expected execution paths using `into_iter()`, the size of the
        // index stack is checked in advance to make sure this condition doesn't occur.
        // However, it's possible to happen if the user manually calls `next_path`.
        if state.is_empty() {
            unreachable!("Index stack too small");
        }

        if state[0] < N {
            // Add the array index to the topic name.
            if write!(path, "{}", state[0]).is_err() {
                unreachable!("Topic buffer too short");
            }

            state[0] += 1;
            true
        } else {
            false
        }
    }
}
