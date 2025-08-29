//! `TreeSerialize`/`TreeDeserialize` with `postcard`.
//!
//! ```
//! use ::postcard::{de_flavors::Slice, ser_flavors::AllocVec};
//! use miniconf::{postcard, Leaf, Packed, Tree, TreeKey};
//!
//! #[derive(Tree, Default, PartialEq, Debug)]
//! struct S {
//!     foo: Leaf<u32>,
//!     bar: [Leaf<u16>; 2],
//! };
//!
//! let source = S {
//!     foo: 9.into(),
//!     bar: [7.into(), 11.into()],
//! };
//! let kv: Vec<_> = S::nodes::<Packed, 2>()
//!     .map(|p| {
//!         let (p, _node) = p.unwrap();
//!         let v = postcard::get_by_key(&source, p, AllocVec::new()).unwrap();
//!         (p.into_lsb().get(), v)
//!     })
//!     .collect();
//! assert_eq!(kv, [(2, vec![9]), (6, vec![7]), (7, vec![11])]);
//!
//! let mut target = S::default();
//! for (k, v) in kv {
//!     let p = Packed::from_lsb(k.try_into().unwrap());
//!     postcard::set_by_key(&mut target, p, Slice::new(&v[..])).unwrap();
//! }
//! assert_eq!(source, target);
//! ```

use postcard::{de_flavors, ser_flavors, Deserializer, Serializer};

use crate::{IntoKeys, SerDeError, TreeDeserialize, TreeSerialize};

/// Deserialize and set a node value from a `postcard` flavor.
#[inline]
pub fn set_by_key<'de, F: de_flavors::Flavor<'de>>(
    tree: &mut (impl TreeDeserialize<'de> + ?Sized),
    keys: impl IntoKeys,
    flavor: F,
) -> Result<F::Remainder, SerDeError<postcard::Error>> {
    let mut de = Deserializer::from_flavor(flavor);
    tree.deserialize_by_key(keys.into_keys(), &mut de)?;
    de.finalize().map_err(SerDeError::Finalization)
}

/// Get and serialize a node value into a `postcard` flavor.
#[inline]
pub fn get_by_key<F: ser_flavors::Flavor>(
    tree: &(impl TreeSerialize + ?Sized),
    keys: impl IntoKeys,
    flavor: F,
) -> Result<F::Output, SerDeError<postcard::Error>> {
    let mut ser = Serializer { output: flavor };
    tree.serialize_by_key(keys.into_keys(), &mut ser)?;
    ser.output.finalize().map_err(SerDeError::Finalization)
}
