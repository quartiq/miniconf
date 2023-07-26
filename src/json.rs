use crate::{Error, Miniconf, SerDe};

/// Marker struct for the "JSON and `/`" [SerDe] specification.
///
/// Access items with `'/'` as path separator and JSON (from `serde-json`)
/// as serialization/deserialization payload format.
pub struct JsonSlash;

impl<T> SerDe<JsonSlash> for T
where
    T: Miniconf,
{
    const SEPARATOR: char = '/';
    type DeError = serde_json::Error;
    type SerError = serde_json::Error;

    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error<Self::DeError>> {
        let mut de = serde_json::Deserializer::from_slice(data);
        self.set_path(&mut path.split(Self::SEPARATOR).skip(1), &mut de)?;
        de.end()
            .map_err(Error::PostDeserialization)
            .map(|_| data.len())
    }

    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<Self::SerError>> {
        let mut buf = std::io::Cursor::new(data);
        let mut ser = serde_json::Serializer::new(&mut buf);
        self.get_path(&mut path.split(Self::SEPARATOR).skip(1), &mut ser)?;
        Ok(buf.position() as _)
    }
}
