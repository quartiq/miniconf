#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(all(feature = "json-core", feature = "postcard"), doc = include_str!("../README.md"))]
#![cfg_attr(
    not(all(feature = "json-core", feature = "postcard")),
    doc = "Miniconf"
)]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

pub use miniconf_derive::*;
mod tree;
pub use tree::*;
mod array;
mod iter;
pub use iter::*;
mod option;
mod packed;
pub use packed::*;
mod key;
pub use key::*;

#[cfg(feature = "json-core")]
mod json_core;
#[cfg(feature = "json-core")]
pub use json_core::*;

#[cfg(feature = "postcard")]
mod postcard;
#[cfg(feature = "postcard")]
pub use crate::postcard::*;

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
