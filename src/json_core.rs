use crate::{Error, Miniconf};
use serde_json_core::{de, ser};

/// Miniconf with "JSON and `/`".
///
/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization payload format.
pub trait JsonCoreSlash: Miniconf {
    /// Update an element by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set_json(
        &mut self,
        path: &str,
        data: &[u8],
    ) -> core::result::Result<usize, Error<de::Error>>;

    /// Retrieve a serialized value by path.
    ///
    /// # Args
    /// * `path` - The path to the element.
    /// * `data` - The buffer to serialize the data into.
    ///
    /// # Returns
    /// The number of bytes used in the `data` buffer or an [Error].
    fn get_json(
        &self,
        path: &str,
        data: &mut [u8],
    ) -> core::result::Result<usize, Error<ser::Error>>;

    /// Update an element by indices.
    ///
    /// # Args
    /// * `indices` - The indices to the element.
    /// * `data` - The serialized data making up the content.
    ///
    /// # Returns
    /// The number of bytes consumed from `data` or an [Error].
    fn set_json_by_index(
        &mut self,
        indices: &[usize],
        data: &[u8],
    ) -> core::result::Result<usize, Error<de::Error>>;

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
    ) -> core::result::Result<usize, Error<ser::Error>>;
}

impl<T: Miniconf> JsonCoreSlash for T {
    fn set_json(&mut self, path: &str, data: &[u8]) -> Result<usize, Error<de::Error>> {
        let mut de = de::Deserializer::new(data);
        self.set_by_key(path.split('/').skip(1), &mut de)?;
        de.end().map_err(Error::PostDeserialization)
    }

    fn get_json(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<ser::Error>> {
        let mut ser = ser::Serializer::new(data);
        self.get_by_key(path.split('/').skip(1), &mut ser)?;
        Ok(ser.end())
    }

    fn set_json_by_index(
        &mut self,
        indices: &[usize],
        data: &[u8],
    ) -> Result<usize, Error<de::Error>> {
        let mut de = de::Deserializer::new(data);
        self.set_by_key(indices.iter().copied(), &mut de)?;
        de.end().map_err(Error::PostDeserialization)
    }

    fn get_json_by_index(
        &self,
        indices: &[usize],
        data: &mut [u8],
    ) -> Result<usize, Error<ser::Error>> {
        let mut ser = ser::Serializer::new(data);
        self.get_by_key(indices.iter().copied(), &mut ser)?;
        Ok(ser.end())
    }
}

// These allow unifying serde error information to make writing examples
// and tests easier. Doing this conversion is optional.
// #[cfg(any(test, doctest))]
impl From<Error<ser::Error>> for Error<de::Error> {
    fn from(value: Error<ser::Error>) -> Self {
        match value {
            Error::NotFound(i) => Self::NotFound(i),
            Error::TooLong(i) => Self::TooLong(i),
            Error::Absent(i) => Self::Absent(i),
            Error::TooShort(i) => Self::TooShort(i),
            Error::PostDeserialization(_) => Error::PostDeserialization(de::Error::CustomError),
            Error::Inner(_) => Self::Inner(de::Error::CustomError),
        }
    }
}
