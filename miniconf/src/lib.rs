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
mod json_core;
#[cfg(feature = "json-core")]
pub use json_core::*;

#[cfg(feature = "postcard")]
mod postcard;
#[cfg(feature = "postcard")]
pub use crate::postcard::*;

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{Deserialize, Deserializer, Serialize, Serializer};
