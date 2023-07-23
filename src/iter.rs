use super::Miniconf;
use core::marker::PhantomData;
use heapless::String;

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniconfIter<M: ?Sized, const L: usize, const TS: usize> {
    /// Zero-size marker field to allow being generic over M and gaining access to M.
    marker: PhantomData<M>,

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

    separator: char,
}

impl<M: ?Sized, const L: usize, const TS: usize> MiniconfIter<M, L, TS> {
    pub fn new(count: Option<usize>, separator: char) -> Self {
        Self {
            count,
            separator,
            marker: PhantomData,
            state: [0; L],
        }
    }
}

impl<M: Miniconf + ?Sized, const L: usize, const TS: usize> Iterator for MiniconfIter<M, L, TS> {
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::new();

        if M::next_path(&mut self.state, &mut path, self.separator).unwrap() {
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
