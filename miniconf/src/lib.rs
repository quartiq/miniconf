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
mod option;
// mod tuple;
mod iter;
pub use iter::*;
mod leaf;
pub use leaf::*;
mod walk;
pub use walk::*;

#[cfg(feature = "derive")]
pub use miniconf_derive::*;

#[cfg(feature = "json-core")]
pub mod json;

#[cfg(feature = "postcard")]
pub mod postcard;

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{Deserialize, Deserializer, Serialize, Serializer};
