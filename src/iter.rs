use crate::{Error, Miniconf, Ok, SliceShort};
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
    pub(crate) fn new(separator: &'a str) -> core::result::Result<Self, Error<SliceShort>> {
        let meta = M::metadata();
        if L < meta.max_depth {
            return Err(Error::Inner(SliceShort));
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
            return match M::path(&mut self.state.iter().copied(), &mut path, self.separator) {
                // Not having consumed any name/index, the only possible case here is a bare option.
                // And that can not return `NotFound`.
                Err(Error::NotFound(0)) => unreachable!(),
                // Iteration done
                Err(Error::NotFound(1)) => {
                    debug_assert_eq!(self.count.unwrap_or_default(), 0);
                    None
                }
                // Node not found at depth: reset current index, increment parent index, then retry
                Err(Error::NotFound(depth)) => {
                    path = Self::Item::default();
                    self.state[depth - 1] = 0;
                    self.state[depth - 2] += 1;
                    continue;
                }
                // Iteration done for a bare Option
                Ok(Ok::Leaf(0)) if self.count == Some(0) => None,
                // Root is `Leaf`: bare Option.
                // Since there is no way to end iteration by triggering `NotFound` on a bare Option,
                // we force the count to Some(0) and trigger on that (see above).
                Ok(Ok::Leaf(0)) => {
                    debug_assert_eq!(self.count.unwrap_or(1), 1);
                    self.count = Some(0);
                    Some(path)
                }
                // Non-root leaf: advance at current depth
                Ok(Ok::Leaf(depth)) => {
                    self.count = self.count.map(|c| c - 1);
                    self.state[depth - 1] += 1;
                    Some(path)
                }
                // If we end at a leaf node, the state array is too small.
                Ok(Ok::Internal(_depth)) => panic!("State too small"),
                Err(e) => panic!("{e:?}"),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}
