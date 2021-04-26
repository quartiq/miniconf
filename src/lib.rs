#![no_std]

pub use serde::de::{Deserialize, DeserializeOwned};
pub use serde_json_core;

pub use derive_miniconf::{Miniconf, MiniconfAtomic};

#[derive(Debug, PartialEq)]
pub enum Error {
    NameNotFound,
    NameTooLong,
    NameTooShort,
    AtomicUpdateRequired,
    Deserialization(serde_json_core::de::Error),
    BadIndex,
    IdTooLong,
}

impl From<serde_json_core::de::Error> for Error {
    fn from(err: serde_json_core::de::Error) -> Error {
        Error::Deserialization(err)
    }
}

pub trait Miniconf {
    fn string_set(
        &mut self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error>;
}

/// Convenience function to update settings directly from a string path and data.
///
/// # Note
/// When using prefixes on the path, it is often simpler to call
/// `Settings::string_set(path.peekable(), data)` directly.
///
/// # Args
/// * `settings` - The settings to update
/// * `path` - The path to update within `settings`.
/// * `data` - The serialized data making up the contents of the configured value.
///
/// # Returns
/// The result of the configuration operation.
pub fn update<T: Miniconf>(settings: &mut T, path: &str, data: &[u8]) -> Result<(), Error> {
    settings.string_set(path.split('/').peekable(), data)
}

macro_rules! impl_single {
    ($x:ty) => {
        impl Miniconf for $x {
            fn string_set(
                &mut self,
                mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
                value: &[u8],
            ) -> Result<(), Error> {
                if topic_parts.peek().is_some() {
                    return Err(Error::NameTooLong);
                }
                *self = serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    };
}

macro_rules! impl_array {
    ($($N:literal),*) => {
      $(
        impl<T> Miniconf for [T; $N]
        where
            T: Miniconf + core::marker::Copy + DeserializeOwned,
        {
            fn string_set(
                &mut self,
                mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
                value: &[u8],
            ) -> Result<(), Error> {
                let next = topic_parts.next();
                if next.is_none() {
                    return Err(Error::NameTooShort);
                }

                // Parse what should be the index value
                let i: usize = serde_json_core::from_str(next.unwrap()).or(Err(Error::BadIndex))?.0;

                if i >= self.len() {
                    return Err(Error::BadIndex)
                }

                self[i].string_set(topic_parts, value)?;

                Ok(())
            }
        }
      )*
    }
}

// This is needed until const generics is stabilized https://github.com/rust-lang/rust/issues/44580
impl_array!(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32
);

// Implement trait for the primitive types
impl_single!(u8);
impl_single!(u16);
impl_single!(u32);
impl_single!(u64);

impl_single!(i8);
impl_single!(i16);
impl_single!(i32);
impl_single!(i64);

impl_single!(f32);
impl_single!(f64);

impl_single!(usize);
impl_single!(bool);
