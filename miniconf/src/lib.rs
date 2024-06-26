#![cfg_attr(not(any(test, doctest)), no_std)]
#![cfg_attr(feature = "json-core", doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "json-core"), doc = "miniconf")]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

#[cfg(feature = "derive")]
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
mod jsonpath;
pub use jsonpath::*;
mod error;
pub use error::*;

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

/// Returns the number of digits required to format an integer less than `x`.
pub const fn digits<const BASE: usize>(x: usize) -> usize {
    let mut max = BASE;
    let mut digits = 1;

    while x > max {
        max *= BASE;
        digits += 1;
    }
    digits
}
