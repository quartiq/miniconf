use super::Miniconf;
use heapless::String;

pub struct MiniconfIter<'a, Settings: Miniconf + ?Sized, const TS: usize> {
    pub(crate) settings: &'a Settings,
    pub(crate) state: &'a mut [usize],
}

impl<'a, Settings: Miniconf + ?Sized, const TS: usize> Iterator for MiniconfIter<'a, Settings, TS> {
    type Item = String<TS>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut topic_buffer: String<TS> = String::new();

        if self
            .settings
            .recursive_iter(&mut self.state, &mut topic_buffer)
            .is_some()
        {
            Some(topic_buffer)
        } else {
            None
        }
    }
}
