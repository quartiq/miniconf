use serde_json_core::{de, ser};

use crate::{Error, IntoKeys, Path, TreeDeserialize, TreeSerialize};

/// `TreeSerialize`/`TreeDeserialize` with "JSON and `/`".
///
/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization payload format.
///
/// Paths used here are reciprocal to `TreeKey::lookup::<Path<_, '/'>, _>(...)`/
/// `TreeKey::nodes::<Path<_, '/'>>()`.
///
/// ```
/// use miniconf::{JsonCoreSlash, Tree};
/// #[derive(Tree, Default)]
/// struct S {
///     foo: u32,
///     #[tree(depth=1)]
///     bar: [u16; 2],
/// };
/// let mut s = S::default();
/// s.set_json("/bar/1", b"9").unwrap();
/// assert_eq!(s.bar[1], 9);
/// let mut buf = [0u8; 10];
/// let len = s.get_json("/bar/1", &mut buf[..]).unwrap();
/// assert_eq!(&buf[..len], b"9");
/// ```
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
    /// * `path` - The path to the node. Everything before the first `'/'` is ignored.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>>;

    /// Update a node by key.
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
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json_by_key<K: IntoKeys>(
        &self,
        keys: K,
        data: &mut [u8],
    ) -> Result<usize, Error<ser::Error>>;
}

impl<'de, T: TreeSerialize<Y> + TreeDeserialize<'de, Y> + ?Sized, const Y: usize>
    JsonCoreSlash<'de, Y> for T
{
    fn set_json(&mut self, path: &str, data: &'de [u8]) -> Result<usize, Error<de::Error>> {
        self.set_json_by_key(&Path::<_, '/'>::from(path), data)
    }

    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>> {
        self.get_json_by_key(&Path::<_, '/'>::from(path), data)
    }

    fn set_json_by_key<K: IntoKeys>(
        &mut self,
        keys: K,
        data: &'de [u8],
    ) -> Result<usize, Error<de::Error>> {
        let mut de: de::Deserializer<'_, '_> = de::Deserializer::new(data, None);
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

/// Shorthand for owned deserialization through [`JsonCoreSlash`].
pub trait JsonCoreSlashOwned<const Y: usize = 1>: for<'de> JsonCoreSlash<'de, Y> {}
impl<T, const Y: usize> JsonCoreSlashOwned<Y> for T where T: for<'de> JsonCoreSlash<'de, Y> {}
