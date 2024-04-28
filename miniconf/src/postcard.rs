use crate::{Error, IntoKeys, TreeDeserialize, TreeSerialize};
use postcard::{de_flavors, ser_flavors, Deserializer, Serializer};

/// Miniconf with `postcard`.
pub trait Postcard<'de, const Y: usize = 1>: TreeSerialize<Y> + TreeDeserialize<'de, Y> {
    /// Deserialize and set a node value from a `postcard` flavor.
    ///
    /// ```
    /// # use miniconf::{Tree, TreeKey, Postcard, Packed};
    /// # use postcard::de_flavors::Slice;
    /// #[derive(Tree, Default)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 3],
    /// };
    /// let mut s = S::default();
    /// let p = Packed::from_lsb(0b1_1_10.try_into().unwrap());
    /// let r = s.set_postcard_by_key(p, Slice::new(&[7])).unwrap();
    /// assert_eq!(s.bar[2], 7);
    /// assert!(r.is_empty());
    /// ```
    fn set_postcard_by_key<K: IntoKeys, F: de_flavors::Flavor<'de>>(
        &mut self,
        keys: K,
        flavor: F,
    ) -> Result<F::Remainder, Error<postcard::Error>>;

    /// Get and serialize a node value into a `postcard` flavor.
    ///
    /// ```
    /// # use miniconf::{Tree, TreeKey, Postcard, Packed};
    /// # use postcard::ser_flavors::Slice;
    /// #[derive(Tree, Default)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 3],
    /// };
    /// let mut s = S::default();
    /// s.bar[2] = 7;
    /// let p = Packed::from_lsb(0b1_1_10.try_into().unwrap());
    /// let mut buf = [0; 1];
    /// let o = s.get_postcard_by_key(p, Slice::new(&mut buf[..])).unwrap();
    /// assert_eq!(o, [7]);
    /// ```
    fn get_postcard_by_key<K: IntoKeys, F: ser_flavors::Flavor>(
        &mut self,
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
        &mut self,
        keys: K,
        flavor: F,
    ) -> Result<F::Output, Error<postcard::Error>> {
        let mut ser = Serializer { output: flavor };
        self.serialize_by_key(keys.into_keys(), &mut ser)?;
        ser.output.finalize().map_err(Error::Finalization)
    }
}
