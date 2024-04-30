use crate::{Error, IntoKeys, TreeDeserialize, TreeSerialize};
use postcard::{de_flavors, ser_flavors, Deserializer, Serializer};

/// `TreeSerialize`/`TreeDeserialize` with `postcard`.
///
/// ```
/// # #[cfg(feature = "std")]
/// # {
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
/// let kv: Vec<_> = S::iter_packed().map(|p| {
///     let p = p.unwrap();
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
/// # }
/// ```
pub trait Postcard<'de, const Y: usize = 1>: TreeSerialize<Y> + TreeDeserialize<'de, Y> {
    /// Deserialize and set a node value from a `postcard` flavor.
    ///
    /// ```
    /// # use miniconf::{Tree, TreeKey, Postcard, Packed};
    /// use postcard::de_flavors::Slice;
    /// #[derive(Tree, Default)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 3],
    /// };
    /// let mut s = S::default();
    /// let p = Packed::from_lsb(0b1_1_10.try_into().unwrap());
    /// let r = s.set_postcard_by_key(p, Slice::new(&[7u8])).unwrap();
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
    /// use postcard::ser_flavors::Slice;
    /// #[derive(Tree, Default)]
    /// struct S {
    ///     foo: u32,
    ///     #[tree(depth=1)]
    ///     bar: [u16; 3],
    /// };
    /// let mut s = S::default();
    /// s.bar[2] = 7;
    /// let p = Packed::from_lsb(0b1_1_10.try_into().unwrap());
    /// let mut buf = [0u8; 1];
    /// let o = s.get_postcard_by_key(p, Slice::new(&mut buf[..])).unwrap();
    /// assert_eq!(o, [7]);
    /// ```
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
