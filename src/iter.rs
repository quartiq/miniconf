use crate::{Error, Miniconf, SliceShort};
use core::{fmt::Write, iter::FusedIterator, marker::PhantomData};

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathIter<'a, M: ?Sized, const L: usize, P> {
    /// Zero-size markers to allow being generic over M/P (by constraining the type parameters).
    m: PhantomData<M>,
    p: PhantomData<P>,

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

    /// The separator before each name.
    separator: &'a str,
}

impl<'a, M, const L: usize, P> PathIter<'a, M, L, P>
where
    M: Miniconf + ?Sized,
{
    pub(crate) fn new(separator: &'a str) -> Result<Self, SliceShort> {
        let meta = M::metadata();
        if L < meta.max_depth {
            return Err(SliceShort);
        }
        let mut s = Self::new_unchecked(separator);
        s.count = Some(meta.count);
        Ok(s)
    }

    pub(crate) fn new_unchecked(separator: &'a str) -> Self {
        Self {
            count: None,
            separator,
            state: [0; L],
            m: PhantomData,
            p: PhantomData,
        }
    }
}

impl<'a, M, const L: usize, P> Iterator for PathIter<'a, M, L, P>
where
    M: Miniconf + ?Sized,
    P: Write + Default,
{
    type Item = Result<P, Error<core::fmt::Error>>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = P::default();

        loop {
            return match M::path(self.state, &mut path, self.separator) {
                // Out of valid indices at the root: iteration done
                Err(Error::NotFound(1)) => {
                    debug_assert_eq!(self.count.unwrap_or_default(), 0);
                    None
                }
                // Node not found at depth: reset current index, increment parent index,
                // then retry path()
                Err(Error::NotFound(depth @ 2..)) => {
                    path = P::default();
                    self.state[depth - 1] = 0;
                    self.state[depth - 2] += 1;
                    continue;
                }
                // Found a leaf at the root: bare Option
                // Since there is no way to end iteration by triggering `NotFound` on a bare Option,
                // we force the count to Some(0) and trigger on that.
                Ok(0) => {
                    if self.count == Some(0) {
                        None
                    } else {
                        debug_assert_eq!(self.count.unwrap_or(1), 1);
                        self.count = Some(0);
                        Some(Ok(path))
                    }
                }
                // Non-root leaf: advance index at current depth
                Ok(depth) => {
                    self.count = self.count.map(|c| c - 1);
                    self.state[depth - 1] += 1;
                    Some(Ok(path))
                }
                // If we end at a leaf node, the state array is too small.
                Err(e @ (Error::TooShort(_) | Error::Inner(_))) => Some(Err(e)),
                // Note(`NotFound(0)`) Not having consumed any name/index, the only possible case
                // is a bare `Miniconf` thing that does not add any hierarchy, e.g. `Option`.
                // That however can not return `NotFound` at all.
                // No other errors can be returned by traverse_by_key()
                _ => unreachable!(),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}

impl<'a, M, const L: usize, P> FusedIterator for PathIter<'a, M, L, P>
where
    M: Miniconf,
    P: core::fmt::Write + Default,
{
}
