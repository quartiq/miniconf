use crate::{Error, Miniconf, SliceShort};
use core::{fmt::Write, marker::PhantomData};

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

impl<'a, M: Miniconf + ?Sized, const L: usize, P> PathIter<'a, M, L, P> {
    pub(crate) fn new(separator: &'a str) -> core::result::Result<Self, SliceShort> {
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
    type Item = P;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::default();

        loop {
            return match M::path(self.state, &mut path, self.separator) {
                // Not having consumed any name/index, the only possible case here is a bare option.
                // And that can not return `NotFound`.
                Err(Error::NotFound(0)) => unreachable!(),
                // Out of valid indices at the root: iteration done
                Err(Error::NotFound(1)) => {
                    debug_assert_eq!(self.count.unwrap_or_default(), 0);
                    None
                }
                // Node not found at depth: reset current index, increment parent index,
                // then retry path()
                Err(Error::NotFound(depth)) => {
                    path = Self::Item::default();
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
                        Some(path)
                    }
                }
                // Non-root leaf: advance index at current depth
                Ok(depth) => {
                    self.count = self.count.map(|c| c - 1);
                    self.state[depth - 1] += 1;
                    Some(path)
                }
                // If we end at a leaf node, the state array is too small.
                Err(Error::TooShort(_depth)) => panic!("Path iteration state too small"),
                Err(Error::Inner(e @ core::fmt::Error)) => panic!("Path write error: {e:?}"),
                _ => unreachable!(),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}
