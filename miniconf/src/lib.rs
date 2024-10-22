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
mod tuple;
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


struct S<T>(Leaf<T>);
impl<T> TreeKey for S<T> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        unimplemented!()
    }
    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
        where
            K: Keys,
            F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E> {
        unimplemented!()
    }
}
impl<T> TreeSerialize for S<T> {
    fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
        where
            K: Keys,
            S: Serializer {
        unimplemented!()
    }
}
