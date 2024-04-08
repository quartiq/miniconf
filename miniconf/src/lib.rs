#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(feature = "json-core", doc = include_str!("../../README.md"))]
#![cfg_attr(not(feature = "json-core"), doc = "Miniconf")]
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

#[cfg(feature = "json-core")]
mod json_core;
#[cfg(feature = "json-core")]
pub use json_core::*;

#[cfg(feature = "mqtt-client")]
mod mqtt_client;
#[cfg(feature = "mqtt-client")]
pub use mqtt_client::*;

#[cfg(feature = "mqtt-client")]
pub use minimq;

// re-export for proc-macro
#[doc(hidden)]
pub use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
