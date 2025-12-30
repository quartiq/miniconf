#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![cfg_attr(all(feature = "derive", feature = "json-core"), doc = include_str!("../README.md"))]
#![cfg_attr(not(all(feature = "derive", feature = "json-core")), doc = "Miniconf")]

mod error;
pub use error::*;
mod key;
pub use key::*;
mod schema;
pub use schema::*;
mod shape;
pub use shape::*;
mod packed;
pub use packed::*;
mod jsonpath;
pub use jsonpath::*;
mod tree;
pub use tree::*;
mod iter;
pub use iter::*;
mod impls;
pub use impls::*;

#[cfg(feature = "derive")]
pub use miniconf_derive::*;

#[cfg(feature = "json-core")]
pub mod json_core;

#[cfg(feature = "json")]
pub mod json;

#[cfg(feature = "postcard")]
pub mod postcard;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "trace")]
pub mod trace;

#[cfg(feature = "schema")]
pub mod json_schema;

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeSeed};
