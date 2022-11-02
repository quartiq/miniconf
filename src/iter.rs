use super::Miniconf;
use core::marker::PhantomData;
use heapless::String;

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniconfIter<M: ?Sized, const L: usize, const TS: usize> {
    marker: PhantomData<M>,
    state: [usize; L],
    count: Option<usize>,
}

impl<M: ?Sized, const L: usize, const TS: usize> Default for MiniconfIter<M, L, TS> {
    fn default() -> Self {
        MiniconfIter {
            marker: PhantomData,
            state: [0; L],
            count: None,
        }
    }
}

impl<M: ?Sized, const L: usize, const TS: usize> MiniconfIter<M, L, TS> {
    pub fn new(count: Option<usize>) -> Self {
        Self {
            count,
            ..Default::default()
        }
    }
}

impl<M: Miniconf + ?Sized, const L: usize, const TS: usize> Iterator for MiniconfIter<M, L, TS> {
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::new();

        if M::next_path(&mut self.state, &mut path).unwrap() {
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
