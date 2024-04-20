use crate::{Error, IntoKeys, TreeDeserialize, TreeSerialize};
use serde_json_core::{de, ser};

/// Miniconf with "JSON and `/`".
///
/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization payload format.
pub trait JsonCoreSlash<'de, const Y: usize = 1>:
    TreeSerialize<Y> + TreeDeserialize<'de, Y>
{
    /// Update a node by path.
    ///
    /// # Args
    /// * `path` - The path to the node. Everything before the first `'/'` is ignored.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set_json(&mut self, path: &str, data: &'de [u8]) -> Result<usize, Error<de::Error>>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the node.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>>;

    /// Update a node by key.
    ///
    /// # Args
    /// * `keys` - The `Keys` to the node.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set_json_by_key<K: IntoKeys>(
        &mut self,
        keys: K,
        data: &'de [u8],
    ) -> Result<usize, Error<de::Error>>;

    /// Retrieve a serialized value by key.
    ///
    /// # Args
    /// * `keys` - The `Keys` to the node.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json_by_key<K: IntoKeys>(
        &self,
        keys: K,
        data: &mut [u8],
    ) -> Result<usize, Error<ser::Error>>;
}

impl<'de, T: TreeSerialize<Y> + TreeDeserialize<'de, Y>, const Y: usize> JsonCoreSlash<'de, Y>
    for T
{
    fn set_json(&mut self, path: &str, data: &'de [u8]) -> Result<usize, Error<de::Error>> {
        self.set_json_by_key(path.split('/').skip(1), data)
    }

    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>> {
        self.get_json_by_key(path.split('/').skip(1), data)
    }

    fn set_json_by_key<K: IntoKeys>(
        &mut self,
        keys: K,
        data: &'de [u8],
    ) -> Result<usize, Error<de::Error>> {
        let mut de = de::Deserializer::new(data);
        self.deserialize_by_key(keys.into_keys(), &mut de)?;
        de.end().map_err(Error::Finalization)
    }

    fn get_json_by_key<K: IntoKeys>(
        &self,
        keys: K,
        data: &mut [u8],
    ) -> Result<usize, Error<ser::Error>> {
        let mut ser = ser::Serializer::new(data);
        self.serialize_by_key(keys.into_keys(), &mut ser)?;
        Ok(ser.end())
    }
}
