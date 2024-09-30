#![no_std]
#![doc = include_str!("../README.md")]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

mod error;
pub use error::*;
mod key;
pub use key::*;
mod node;
pub use node::*;
mod packed;
pub use packed::*;
mod jsonpath;
pub use jsonpath::*;
mod tree;
pub use tree::*;
mod array;
mod iter;
mod option;
pub use iter::*;

#[cfg(feature = "derive")]
pub use miniconf_derive::*;

#[cfg(feature = "json-core")]
pub mod json;
#[cfg(feature = "json-core")]
#[allow(deprecated)]
pub use json::{JsonCoreSlash, JsonCoreSlashOwned};

#[cfg(feature = "postcard")]
pub mod postcard;
#[cfg(feature = "postcard")]
#[allow(deprecated)]
pub use crate::postcard::{Postcard, PostcardOwned};

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{Deserialize, Deserializer, Serialize, Serializer};
