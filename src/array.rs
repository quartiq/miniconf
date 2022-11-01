//! Array support
//!
//! # Design
//! Miniconf supports lists of items in configurable structures using two forms. For the
//! [`DeferredArray`], all of the contents of the array are accessed as `Miniconf` settings trees.
//!
//! For standard arrays of [T; N] form, the individual elements of the array are accessed as atomic
//! values (i.e. a single Miniconf item).
//!
//! The type you should use depends on what data is contained in your array. If your array contains
//! trees of settings, you should use [`DeferredArray`]. However, if each element in your list is
//! individually configurable as a single value (e.g. a list of u32), then you should use a
//! standard [T; N] array.
//!
//! ## Atomic Array Access
//!
//! By default, arrays have an implied `#[miniconf(defer)]` attached to them. That is, each element
//! is individually accessible. This is normally the desired mode of operation, but there are cases
//! where the user may want to update the entire array in a single call. To do this, you can
//! annodate a [T; N] array with `#[miniconf(atomic)]`.
//!
//! When `#[miniconf(atomic)]` is attributed to an array, the entire array must be accessed as a
//! single element. All values will be simultaneously read and written.
use super::{Error, Miniconf, MiniconfMetadata};

use core::fmt::Write;

pub struct DeferredArray<T, const N: usize>(pub [T; N]);

impl<T, const N: usize> core::ops::Deref for DeferredArray<T, N> {
    type Target = [T; N];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T, const N: usize> core::ops::DerefMut for DeferredArray<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Default + Copy, const N: usize> Default for DeferredArray<T, N> {
    fn default() -> Self {
        Self([T::default(); N])
    }
}

impl<T: core::fmt::Debug, const N: usize> core::fmt::Debug for DeferredArray<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq, const N: usize> PartialEq<[T; N]> for DeferredArray<T, N> {
    fn eq(&self, other: &[T; N]) -> bool {
        self.0.eq(other)
    }
}

impl<T: PartialEq, const N: usize> PartialEq for DeferredArray<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Clone, const N: usize> Clone for DeferredArray<T, N> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy, const N: usize> Copy for DeferredArray<T, N> {}

impl<T, const N: usize> DeferredArray<T, N> {
    fn index(&self, next: Option<&str>) -> Result<usize, Error> {
        let next = next.ok_or(Error::PathTooShort)?;

        // Parse what should be the index value
        let i: usize = serde_json_core::from_str(next).or(Err(Error::BadIndex))?.0;

        if i >= self.0.len() {
            Err(Error::BadIndex)
        } else {
            Ok(i)
        }
    }
}

const fn digits(x: usize) -> usize {
    let mut n = 10;
    let mut num_digits = 1;

    while x >= n {
        n *= 10;
        num_digits += 1;
    }
    num_digits
}

impl<T: Miniconf, const N: usize> Miniconf for DeferredArray<T, N> {
    fn set_path(
        &mut self,
        mut path_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        let i = self.index(path_parts.next())?;

        self.0[i].set_path(path_parts, value)?;

        Ok(())
    }

    fn get_path(
        &self,
        mut path_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        let i = self.index(path_parts.next())?;

        self.0[i].get_path(path_parts, value)
    }

    fn metadata(&self) -> MiniconfMetadata {
        // First, figure out how many digits the maximum index requires when printing.

        let mut meta = self.0[0].metadata();

        // If the sub-members have topic size, we also need to include an additional character for
        // the path separator. This is ommitted if the sub-members have no topic (e.g. fundamental
        // types, enums).
        if meta.max_topic_size > 0 {
            meta.max_topic_size += 1;
        }

        meta.max_topic_size += digits(N - 1);
        meta.max_depth += 1;

        meta
    }

    fn next_path<const TS: usize>(
        &self,
        state: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> bool {
        let original_length = topic.len();

        // Note(unreachable): During expected execution paths using `into_iter()`, the size of the
        // index stack is checked in advance to make sure this condition doesn't occur.
        // However, it's possible to happen if the user manually calls `next_path`.
        if state.is_empty() {
            unreachable!("Index stack too small");
        }

        while state[0] < N {
            // Add the array index to the topic name.
            if topic.len() > 0 && topic.push('/').is_err() {
                unreachable!("Topic buffer too short");
            }

            if write!(topic, "{}", state[0]).is_err() {
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
    fn index(
        &self,
        path_parts: core::iter::Peekable<core::str::Split<char>>,
    ) -> Result<usize, Error>;
}

impl<T, const N: usize> IndexLookup for [T; N] {
    fn index(
        &self,
        mut path_parts: core::iter::Peekable<core::str::Split<char>>,
    ) -> Result<usize, Error> {
        let next = path_parts.next().ok_or(Error::PathTooShort)?;

        if path_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        // Parse what should be the index value
        Ok(serde_json_core::from_str(next)
            .map_err(|_| Error::BadIndex)?
            .0)
    }
}

impl<T: crate::Serialize + crate::DeserializeOwned, const N: usize> Miniconf for [T; N] {
    fn set_path(
        &mut self,
        path_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        let i = self.index(path_parts)?;
        let ele = <[T]>::get_mut(self, i).ok_or(Error::BadIndex)?;
        *ele = serde_json_core::from_slice(value)?.0;
        Ok(())
    }

    fn get_path(
        &self,
        path_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        let i = self.index(path_parts)?;
        let ele = <[T]>::get(self, i).ok_or(Error::BadIndex)?;
        serde_json_core::to_slice(ele, value).map_err(|_| Error::SerializationFailed)
    }

    fn metadata(&self) -> MiniconfMetadata {
        MiniconfMetadata {
            max_topic_size: digits(N - 1),
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
            if path.len() > 0 && path.push('/').is_err() {
                unreachable!("Topic buffer too short");
            }

            if write!(path, "{}", state[0]).is_err() {
                unreachable!("Topic buffer too short");
            }

            state[0] += 1;
            return true;
        }

        false
    }
}
