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
    const SEPARATOR: &'static str = "/";
    type DeError = de::Error;
    type SerError = ser::Error;

    fn set(&mut self, path: &str, data: &[u8]) -> Result<usize, Error<Self::DeError>> {
        let mut de = de::Deserializer::new(data);
        self.set_by_name(&mut path.split(Self::SEPARATOR).skip(1), &mut de)?;
        de.end().map_err(Error::PostDeserialization)
    }

    fn get(&self, path: &str, data: &mut [u8]) -> Result<usize, Error<Self::SerError>> {
        let mut ser = ser::Serializer::new(data);
        self.get_by_name(&mut path.split(Self::SEPARATOR).skip(1), &mut ser)?;
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
            Error::Internal(i) => Self::Internal(i),
            Error::PostDeserialization(_) => Error::PostDeserialization(de::Error::CustomError),
            Error::Inner(_) => Self::Inner(de::Error::CustomError),
        }
    }
}
