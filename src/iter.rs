use super::{IterError, Metadata, Miniconf, SerDe};
use core::{fmt::Write, marker::PhantomData};

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniconfIter<M: ?Sized, const L: usize, S, P> {
    /// Zero-size marker field to allow being generic over M and gaining access to M.
    miniconf: PhantomData<M>,
    spec: PhantomData<S>,
    path: PhantomData<P>,

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

impl<M: ?Sized, const L: usize, S, P> Default for MiniconfIter<M, L, S, P> {
    fn default() -> Self {
        Self {
            count: None,
            miniconf: PhantomData,
            spec: PhantomData,
            path: PhantomData,
            state: [0; L],
        }
    }
}

impl<M: ?Sized + Miniconf + SerDe<S>, const L: usize, S, P> MiniconfIter<M, L, P, S> {
    pub fn metadata() -> Result<Metadata, IterError> {
        let meta = M::metadata(M::SEPARATOR.len_utf8());
        if L < meta.max_depth {
            return Err(IterError::Depth);
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

impl<M: Miniconf + SerDe<S> + ?Sized, const L: usize, P: Write + Default, S> Iterator
    for MiniconfIter<M, L, P, S>
{
    type Item = P;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::default();

        loop {
            match M::next_path(&self.state, 0, &mut path, M::SEPARATOR) {
                Ok(depth) => {
                    self.count = self.count.map(|c| c - 1);
                    self.state[depth] += 1;
                    return Some(path);
                }
                Err(IterError::Next(0)) => {
                    debug_assert_eq!(self.count.unwrap_or_default(), 0);
                    return None;
                }
                Err(IterError::Next(depth)) => {
                    path = Self::Item::default();
                    self.state[depth] = 0;
                    self.state[depth - 1] += 1;
                }
                e => {
                    e.unwrap();
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}
