//! Read and write leaf values with the Postcard binary format.
//!
//! Collect a tree as compact-key/value records, then replay them into another
//! instance of the same type. This host-side example allocates the record list
//! and payloads; each leaf can also be processed separately in a fixed buffer.
//!
//! ```
//! use ::postcard::{de_flavors::Slice, ser_flavors::AllocVec};
//! use miniconf::{postcard, Packed, Tree, TreeSchema};
//!
//! #[derive(Tree, Default, PartialEq, Debug)]
//! struct S {
//!     foo: u32,
//!     bar: [u16; 2],
//! };
//!
//! let source = S {
//!     foo: 9,
//!     bar: [7, 11],
//! };
//! let kv: Vec<_> = S::SCHEMA.nodes::<Packed, 2>()
//!     .map(|p| {
//!         let p = p.unwrap();
//!         let v = postcard::get_by_key(&source, p, AllocVec::new()).unwrap();
//!         (p.into_lsb().get(), v)
//!     })
//!     .collect();
//!
//! let mut target = S::default();
//! for (k, v) in kv {
//!     let p = Packed::from_lsb(k.try_into().unwrap());
//!     let remaining = postcard::set_by_key(&mut target, p, Slice::new(&v)).unwrap();
//!     assert!(remaining.is_empty());
//! }
//! assert_eq!(source, target);
//! ```
//!
//! Compact keys depend on the tree's schema: these records are not a migration
//! format for changed settings types. For a single leaf without allocation, see
//! the [fixed-buffer example](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/packed.rs).

use postcard::{Deserializer, Serializer, de_flavors, ser_flavors};

use crate::{IntoKeys, Keys, SerdeError, TreeDeserialize, TreeSerialize};

/// Deserialize and set a node value from a `postcard` flavor using a boundary key input.
pub fn set_by_key<'de, F: de_flavors::Flavor<'de>>(
    tree: &mut (impl TreeDeserialize<'de> + ?Sized),
    keys: impl IntoKeys,
    flavor: F,
) -> Result<F::Remainder, SerdeError<postcard::Error>> {
    set_by_keys(tree, keys.into_keys(), flavor)
}

/// Deserialize and set a node value from a `postcard` flavor using a normalized key cursor.
pub fn set_by_keys<'de, F: de_flavors::Flavor<'de>>(
    tree: &mut (impl TreeDeserialize<'de> + ?Sized),
    keys: impl Keys,
    flavor: F,
) -> Result<F::Remainder, SerdeError<postcard::Error>> {
    let mut de = Deserializer::from_flavor(flavor);
    tree.deserialize_by_key(keys, &mut de)?;
    de.finalize().map_err(SerdeError::Finalization)
}

/// Get and serialize a node value into a `postcard` flavor using a boundary key input.
pub fn get_by_key<F: ser_flavors::Flavor>(
    tree: &(impl TreeSerialize + ?Sized),
    keys: impl IntoKeys,
    flavor: F,
) -> Result<F::Output, SerdeError<postcard::Error>> {
    get_by_keys(tree, keys.into_keys(), flavor)
}

/// Get and serialize a node value into a `postcard` flavor using a normalized key cursor.
pub fn get_by_keys<F: ser_flavors::Flavor>(
    tree: &(impl TreeSerialize + ?Sized),
    keys: impl Keys,
    flavor: F,
) -> Result<F::Output, SerdeError<postcard::Error>> {
    let mut ser = Serializer { output: flavor };
    tree.serialize_by_key(keys, &mut ser)?;
    ser.output.finalize().map_err(SerdeError::Finalization)
}
