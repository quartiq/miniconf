//! `TreeSerialize`/`TreeDeserialize` with `postcard`.
//!
//! ```
//! use miniconf::{Tree, TreeKey, postcard, Packed};
//! use ::postcard::{ser_flavors::AllocVec, de_flavors::Slice};
//!
//! #[derive(Tree, Default, PartialEq, Debug)]
//! struct S {
//!     foo: u32,
//!     #[tree(depth=1)]
//!     bar: [u16; 2],
//! };
//!
//! let source = S { foo: 9, bar: [7, 11] };
//! let kv: Vec<_> = S::nodes::<Packed>().map(|p| {
//!     let (p, _node) = p.unwrap();
//!     let v = postcard::get_by_key(&source, p, AllocVec::new()).unwrap();
//!     (p.into_lsb().get(), v)
//! }).collect();
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

/// Postcard Tree
#[deprecated]
pub trait Postcard<'de, const Y: usize = 1>: TreeSerialize<Y> + TreeDeserialize<'de, Y> {
    /// Deserialize and set a node value from a `postcard` flavor.
    fn set_postcard_by_key<K: IntoKeys, F: de_flavors::Flavor<'de>>(
        &mut self,
        keys: K,
        flavor: F,
    ) -> Result<F::Remainder, Error<postcard::Error>>;

    /// Get and serialize a node value into a `postcard` flavor.
    fn get_postcard_by_key<K: IntoKeys, F: ser_flavors::Flavor>(
        &self,
        keys: K,
        flavor: F,
    ) -> Result<F::Output, Error<postcard::Error>>;
}

#[allow(deprecated)]
impl<'de, T: TreeSerialize<Y> + TreeDeserialize<'de, Y> + ?Sized, const Y: usize> Postcard<'de, Y>
    for T
{
    fn set_postcard_by_key<K: IntoKeys, F: de_flavors::Flavor<'de>>(
        &mut self,
        keys: K,
        flavor: F,
    ) -> Result<F::Remainder, Error<postcard::Error>> {
        set_by_key(self, keys, flavor)
    }

    fn get_postcard_by_key<K: IntoKeys, F: ser_flavors::Flavor>(
        &self,
        keys: K,
        flavor: F,
    ) -> Result<F::Output, Error<postcard::Error>> {
        get_by_key(self, keys, flavor)
    }
}

/// Shorthand for owned [`Postcard`].
#[allow(deprecated)]
pub trait PostcardOwned<const Y: usize = 1>: for<'de> Postcard<'de, Y> {}
#[allow(deprecated)]
impl<T, const Y: usize> PostcardOwned<Y> for T where T: for<'de> Postcard<'de, Y> {}

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
