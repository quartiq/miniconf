#![no_std]
pub use serde::de::{Deserialize, DeserializeOwned};
pub use serde_json_core;

pub use derive_stringset::StringSet;

#[derive(Debug)]
pub enum Error {
    NameNotFound,
    NameTooLong,
    NameTooShort,
    Deserialization(serde_json_core::de::Error),
}

impl From<serde_json_core::de::Error> for Error {
    fn from(err: serde_json_core::de::Error) -> Error {
        Error::Deserialization(err)
    }
}

pub trait StringSet {
    fn string_set(
        &mut self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error>;
}

macro_rules! impl_single {
    ($x:ty) => {
        impl StringSet for $x {
            fn string_set(
                &mut self,
                _topic_parts: core::iter::Peekable<core::str::Split<char>>,
                value: &[u8],
            ) -> Result<(), Error> {
                *self = serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    };
}

macro_rules! impl_array {
    ($($N:literal),*) => {
      $(
        impl<T> StringSet for [T; $N]
        where
            T: StringSet + core::marker::Copy + DeserializeOwned,
        {
            fn string_set(
                &mut self,
                _topic_parts: core::iter::Peekable<core::str::Split<char>>,
                value: &[u8],
            ) -> Result<(), Error> {
                let data: [T; $N] = serde_json_core::from_slice(value)?.0;
                self.copy_from_slice(&data);
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
