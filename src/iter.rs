use super::Miniconf;
use core::marker::PhantomData;
use heapless::String;

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniconfIter<M: ?Sized, const L: usize, const TS: usize> {
    pub(crate) marker: PhantomData<M>,
    pub(crate) state: [usize; L],
}

impl<M: ?Sized, const L: usize, const TS: usize> Default for MiniconfIter<M, L, TS> {
    fn default() -> Self {
        MiniconfIter {
            marker: PhantomData,
            state: [0; L],
        }
    }
}

impl<M: Miniconf + ?Sized, const L: usize, const TS: usize> Iterator for MiniconfIter<M, L, TS> {
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::new();

        if M::next_path(&mut self.state, &mut path) {
            Some(path)
        } else {
            None
        }
    }
}
