use super::Miniconf;
use heapless::String;

pub struct MiniconfIter<'a, M: ?Sized, const TS: usize> {
    pub(crate) settings: &'a M,
    pub(crate) state: &'a mut [usize],
}

impl<'a, M: Miniconf + ?Sized, const TS: usize> Iterator for MiniconfIter<'a, M, TS> {
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut topic_buffer: String<TS> = String::new();

        if self
            .settings
            .recurse_paths(self.state, &mut topic_buffer)
            .is_some()
        {
            Some(topic_buffer)
        } else {
            None
        }
    }
}
