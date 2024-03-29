use crate::{Error, TreeDeserialize, TreeSerialize};
use serde_json_core::{de, ser};

/// Miniconf with "JSON and `/`".
///
/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization payload format.
pub trait JsonCoreSlash<'de, const Y: usize = 1>:
    TreeSerialize<Y> + TreeDeserialize<'de, Y>
{
    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element. Everything before the first `'/'` is ignored.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set_json(&mut self, path: &str, data: &'de [u8]) -> Result<usize, Error<de::Error>>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>>;

    /// Update an element by indices.
    ///
    /// # Args
    /// * `indices` - The indices to the element. Everything before the first `'/'` is ignored.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set_json_by_index(
        &mut self,
        indices: &[usize],
        data: &'de [u8],
    ) -> Result<usize, Error<de::Error>>;

    /// Retrieve a serialized value by indices.
    ///
    /// # Args
    /// * `indices` - The indices to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json_by_index(
        &self,
        indices: &[usize],
        data: &mut [u8],
    ) -> Result<usize, Error<ser::Error>>;
}

impl<'de, T: TreeSerialize<Y> + TreeDeserialize<'de, Y>, const Y: usize> JsonCoreSlash<'de, Y>
    for T
{
    fn set_json(&mut self, path: &str, data: &'de [u8]) -> Result<usize, Error<de::Error>> {
        let mut de = de::Deserializer::new(data);
        self.deserialize_by_key(path.split('/').skip(1), &mut de)?;
        de.end().map_err(Error::PostDeserialization)
    }

    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>> {
        let mut ser = ser::Serializer::new(data);
        self.serialize_by_key(path.split('/').skip(1), &mut ser)?;
        Ok(ser.end())
    }

    fn set_json_by_index(
        &mut self,
        indices: &[usize],
        data: &'de [u8],
    ) -> Result<usize, Error<de::Error>> {
        let mut de = de::Deserializer::new(data);
        self.deserialize_by_key(indices.iter().copied(), &mut de)?;
        de.end().map_err(Error::PostDeserialization)
    }

    fn get_json_by_index(
        &self,
        indices: &[usize],
        data: &mut [u8],
    ) -> Result<usize, Error<ser::Error>> {
        let mut ser = ser::Serializer::new(data);
        self.serialize_by_key(indices.iter().copied(), &mut ser)?;
        Ok(ser.end())
    }
}
