use super::{Metadata, Miniconf, SerDe};
use core::marker::PhantomData;
use heapless::String;

/// Errors that occur during iteration over topic paths.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum IterError {
    /// The provided state vector is not long enough.
    PathDepth,

    /// The provided topic length is not long enough.
    PathLength,
}

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniconfIter<M: ?Sized, const L: usize, const TS: usize, S> {
    /// Zero-size marker field to allow being generic over M and gaining access to M.
    miniconf: PhantomData<M>,
    spec: PhantomData<S>,

    /// The iteration state.
    ///
    /// It contains the current field/element index at each path hierarchy level
    /// and needs to be at least as large as the maximum path depth.
    state: [usize; L],

    /// The remaining length of the iterator.
    ///
    /// It is used to provide an exact and trusted [Iterator::size_hint].
    /// C.f. [core::iter::TrustedLen].
    ///
    /// It may be None to indicate unknown length.
    count: Option<usize>,
}

impl<M: ?Sized, const L: usize, const TS: usize, S> Default for MiniconfIter<M, L, TS, S> {
    fn default() -> Self {
        Self {
            count: None,
            miniconf: PhantomData,
            spec: PhantomData,
            state: [0; L],
        }
    }
}

impl<M: ?Sized + Miniconf, const L: usize, const TS: usize, S> MiniconfIter<M, L, TS, S> {
    pub fn metadata() -> Result<Metadata, IterError> {
        let meta = M::metadata();
        if TS < meta.max_length {
            return Err(IterError::PathLength);
        }

        if L < meta.max_depth {
            return Err(IterError::PathDepth);
        }
        Ok(meta)
    }

    pub fn new() -> Result<Self, IterError> {
        let meta = Self::metadata()?;
        Ok(Self {
            count: Some(meta.count),
            ..Default::default()
        })
    }
}

impl<M: Miniconf + SerDe<S> + ?Sized, const L: usize, const TS: usize, S> Iterator
    for MiniconfIter<M, L, TS, S>
{
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::new();

        if M::next_path(&mut self.state, &mut path, M::SEPARATOR).unwrap() {
            self.count = self.count.map(|c| c - 1);
            Some(path)
        } else {
            debug_assert_eq!(self.count.unwrap_or_default(), 0);
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}
