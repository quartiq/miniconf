use super::Miniconf;
use heapless::String;

/// An iterator over the paths in a Miniconf namespace.
pub struct MiniconfIter<'a, M: ?Sized, const TS: usize> {
    pub(crate) namespace: &'a M,
    pub(crate) state: &'a mut [usize],
}

impl<'a, M: Miniconf + ?Sized, const TS: usize> Iterator for MiniconfIter<'a, M, TS> {
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::new();

        if self.namespace.next_path(self.state, &mut path) {
            Some(path)
        } else {
            None
        }
    }
}
