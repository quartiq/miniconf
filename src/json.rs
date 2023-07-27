use crate::{Error, Miniconf};

/// Trait for the "JSON and `/`" [SerDe] specification.
///
/// Access items with `'/'` as path separator and JSON (from `serde-json`)
/// as serialization/deserialization payload format.
pub trait JsonSlash {
    fn set_json(
        &mut self,
        path: &str,
        data: &[u8],
    ) -> core::result::Result<usize, Error<serde_json::Error>>;

    fn get_json(
        &self,
        path: &str,
        data: &mut [u8],
    ) -> core::result::Result<usize, Error<serde_json::Error>>;
}

impl<T: Miniconf> JsonSlash for T {
    fn set_json(&mut self, path: &str, data: &[u8]) -> Result<usize, Error<serde_json::Error>> {
        let mut de = serde_json::Deserializer::from_slice(data);
        self.set_by_name(&mut path.split("/").skip(1), &mut de)?;
        de.end()
            .map_err(Error::PostDeserialization)
            .map(|_| data.len())
    }

    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<serde_json::Error>> {
        let mut buf = std::io::Cursor::new(data);
        let mut ser = serde_json::Serializer::new(&mut buf);
        self.get_by_name(&mut path.split("/").skip(1), &mut ser)?;
        Ok(buf.position() as _)
    }
}
