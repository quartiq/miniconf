use super::{Error, Miniconf, MiniconfMetadata};

pub struct MiniconfOption<T: Miniconf>(pub Option<T>);

impl<T: Miniconf> Miniconf for MiniconfOption<T> {
    fn string_set(
        &mut self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        self.0.as_mut().map_or(Err(Error::PathNotFound), |inner| {
            inner.string_set(topic_parts, value)
        })
    }

    fn string_get(
        &self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        self.0.as_ref().map_or(Err(Error::PathNotFound), |inner| {
            inner.string_get(topic_parts, value)
        })
    }

    fn get_metadata(&self) -> MiniconfMetadata {
        self.0
            .as_ref()
            .map(|value| value.get_metadata())
            .unwrap_or_default()
    }

    fn recurse_paths<const TS: usize>(
        &self,
        index: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> Option<()> {
        self.0
            .as_ref()
            .and_then(|value| value.recurse_paths(index, topic))
    }
}
