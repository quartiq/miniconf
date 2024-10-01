//! `TreeSerialize`/`TreeDeserialize` with `postcard`.
//!
//! ```
//! use ::postcard::{de_flavors::Slice, ser_flavors::AllocVec};
//! use miniconf::{postcard, Packed, Tree, TreeKey};
//!
//! #[derive(Tree, Default, PartialEq, Debug)]
//! struct S {
//!     foo: u32,
//!     #[tree(depth = 1)]
//!     bar: [u16; 2],
//! };
//!
//! let source = S {
//!     foo: 9,
//!     bar: [7, 11],
//! };
//! let kv: Vec<_> = S::nodes::<Packed>()
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

use crate::{Error, IntoKeys, TreeDeserialize, TreeSerialize};

/// Deserialize and set a node value from a `postcard` flavor.
pub fn set_by_key<
    'de,
    T: TreeDeserialize<'de, Y> + ?Sized,
    const Y: usize,
    K: IntoKeys,
    F: de_flavors::Flavor<'de>,
>(
    tree: &mut T,
    keys: K,
    flavor: F,
) -> Result<F::Remainder, Error<postcard::Error>> {
    let mut de = Deserializer::from_flavor(flavor);
    tree.deserialize_by_key(keys.into_keys(), &mut de)?;
    de.finalize().map_err(Error::Finalization)
}

/// Get and serialize a node value into a `postcard` flavor.
pub fn get_by_key<
    T: TreeSerialize<Y> + ?Sized,
    const Y: usize,
    K: IntoKeys,
    F: ser_flavors::Flavor,
>(
    tree: &T,
    keys: K,
    flavor: F,
) -> Result<F::Output, Error<postcard::Error>> {
    let mut ser = Serializer { output: flavor };
    tree.serialize_by_key(keys.into_keys(), &mut ser)?;
    ser.output.finalize().map_err(Error::Finalization)
}
