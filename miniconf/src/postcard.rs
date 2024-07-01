use crate::{Error, IntoKeys, TreeDeserialize, TreeSerialize};
use postcard::{de_flavors, ser_flavors, Deserializer, Serializer};

/// `TreeSerialize`/`TreeDeserialize` with `postcard`.
///
/// ```
/// use miniconf::{Tree, TreeKey, Postcard, Packed};
/// use postcard::{ser_flavors::AllocVec, de_flavors::Slice};
///
/// #[derive(Tree, Default, PartialEq, Debug)]
/// struct S {
///     foo: u32,
///     #[tree(depth=1)]
///     bar: [u16; 2],
/// };
///
/// let source = S { foo: 9, bar: [7, 11] };
/// let kv: Vec<_> = S::nodes::<Packed>().map(|p| {
///     let (p, _node) = p.unwrap();
///     let v = source.get_postcard_by_key(p, AllocVec::new()).unwrap();
///     (p.into_lsb().get(), v)
/// }).collect();
/// assert_eq!(kv, [(2, vec![9]), (6, vec![7]), (7, vec![11])]);
///
/// let mut target = S::default();
/// for (k, v) in kv {
///     let p = Packed::from_lsb(k.try_into().unwrap());
///     target.set_postcard_by_key(p, Slice::new(&v[..])).unwrap();
/// }
/// assert_eq!(source, target);
/// ```
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

impl<'de, T: TreeSerialize<Y> + TreeDeserialize<'de, Y> + ?Sized, const Y: usize> Postcard<'de, Y>
    for T
{
    fn set_postcard_by_key<K: IntoKeys, F: de_flavors::Flavor<'de>>(
        &mut self,
        keys: K,
        flavor: F,
    ) -> Result<F::Remainder, Error<postcard::Error>> {
        let mut de = Deserializer::from_flavor(flavor);
        self.deserialize_by_key(keys.into_keys(), &mut de)?;
        de.finalize().map_err(Error::Finalization)
    }

    fn get_postcard_by_key<K: IntoKeys, F: ser_flavors::Flavor>(
        &self,
        keys: K,
        flavor: F,
    ) -> Result<F::Output, Error<postcard::Error>> {
        let mut ser = Serializer { output: flavor };
        self.serialize_by_key(keys.into_keys(), &mut ser)?;
        ser.output.finalize().map_err(Error::Finalization)
    }
}
