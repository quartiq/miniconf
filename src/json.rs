use crate::{Error, Miniconf, SerDe};
use serde_json_core::{de, ser};

/// Marker struct for the "JSON and `/`" [SerDe] specification.
///
/// Access items with `'/'` as path separator and JSON (from `serde-json-core`)
/// as serialization/deserialization payload format.
pub struct JsonCoreSlash;

impl<T> SerDe<JsonCoreSlash> for T
where
    T: Miniconf,
{
    const SEPARATOR: char = '/';
    type DeError = de::Error;
    type SerError = ser::Error;

    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error<Self::DeError>> {
        let mut de = de::Deserializer::new(data);
        self.set_path(&mut path.split(Self::SEPARATOR).skip(1), &mut de)?;
        de.end().map_err(Error::PostDeserialization)
    }

    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<Self::SerError>> {
        let mut ser = ser::Serializer::new(data);
        self.get_path(&mut path.split(Self::SEPARATOR).skip(1), &mut ser)?;
        Ok(ser.end())
    }
}

// These allow unifying serde error information to make writing examples
// and tests easier. Doing this conversion is optional.
// #[cfg(any(test, doctest))]
impl From<Error<ser::Error>> for Error<de::Error> {
    fn from(value: Error<ser::Error>) -> Self {
        match value {
            Error::BadIndex => Self::BadIndex,
            Error::PathAbsent => Self::PathAbsent,
            Error::PathNotFound => Self::PathNotFound,
            Error::PathTooLong => Self::PathTooLong,
            Error::PathTooShort => Self::PathTooShort,
            Error::PostDeserialization(_) => Error::PostDeserialization(de::Error::CustomError),
            Error::SerDe(_) => Self::SerDe(de::Error::CustomError),
        }
    }
}
