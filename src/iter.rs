use crate::{Error, Miniconf};
use core::{fmt::Write, iter::FusedIterator, marker::PhantomData};

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathIter<'a, M: ?Sized, const Y: usize, P> {
    /// Zero-size markers to allow being generic over M/P (by constraining the type parameters).
    m: PhantomData<M>,
    p: PhantomData<P>,

    /// The iteration state.
    ///
    /// It contains the current field/element index at each path hierarchy level
    /// and needs to be at least as large as the maximum path depth.
    state: [usize; Y],

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

impl<'a, M, const Y: usize, P> PathIter<'a, M, Y, P>
where
    M: Miniconf<Y> + ?Sized,
{
    pub(crate) fn new(separator: &'a str) -> Self {
        let meta = M::metadata();
        assert!(Y == meta.max_depth);
        let mut s = Self::new_unchecked(separator);
        s.count = Some(meta.count);
        s
    }

    pub(crate) fn new_unchecked(separator: &'a str) -> Self {
        Self {
            count: None,
            separator,
            state: [0; Y],
            m: PhantomData,
            p: PhantomData,
        }
    }
}

impl<'a, M, const Y: usize, P> Iterator for PathIter<'a, M, Y, P>
where
    M: Miniconf<Y> + ?Sized,
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
                // Found a leaf at the root: bare Option/newtype
                // Since there is no way to end iteration by hoping for `NotFound` on a bare Option,
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
                Err(e @ Error::Inner(_)) => Some(Err(e)),
                // * NotFound(0) Not having consumed any name/index, the only possible case
                //   is a bare `Miniconf` thing that does not use a key, e.g. `Option`.
                //   That however can not return `NotFound`.
                // * TooShort is excluded by construction.
                // * No other errors can be returned by traverse_by_key()/path()
                _ => unreachable!(),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}

impl<'a, M, const Y: usize, P> FusedIterator for PathIter<'a, M, Y, P>
where
    M: Miniconf<Y>,
    P: core::fmt::Write + Default,
{
}
