use crate::Traversal;

/// Look up top level field names and convert to indices
///
/// This trait is derived together with [`crate::TreeKey`].
///
/// ```
/// use miniconf::{KeyLookup, TreeKey};
/// #[derive(TreeKey)]
/// struct S {
///     foo: u32,
///     bar: [u16; 2],
/// }
/// assert_eq!(S::LEN, 2);
/// assert_eq!(S::NAMES[1], "bar");
/// assert_eq!(S::name_to_index("bar").unwrap(), 1);
/// ```
pub trait KeyLookup {
    /// The number of top-level nodes.
    ///
    /// This is used by `impl Keys for Packed`.
    const LEN: usize;

    /// Field names.
    ///
    /// May be empty if `Self` computes and parses names.
    const NAMES: &'static [&'static str];

    /// Convert a top level node name to a node index.
    ///
    /// The details of the mapping and the `usize` index values
    /// are an implementation detail and only need to be stable at runtime.
    /// This is used by `impl Key for &str`.
    fn name_to_index(value: &str) -> Option<usize>;
}

/// Convert a `&str` key into a node index on a `TreeKey`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find<M: KeyLookup + ?Sized>(&self) -> Option<usize>;
}

// `usize` index as Key
impl Key for usize {
    fn find<M: KeyLookup + ?Sized>(&self) -> Option<usize> {
        Some(*self)
    }
}

// &str name as Key
impl Key for &str {
    fn find<M: KeyLookup + ?Sized>(&self) -> Option<usize> {
        M::name_to_index(self)
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`KeyLookup`] and convert to `usize` index.
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal>;

    /// Finalize the keys, ensure there are no more.
    fn finalize(&mut self) -> bool;

    /// Chain another `Keys` to this one.
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self, U::IntoKeys>
    where
        Self: Sized,
    {
        Chain(self, other.into_keys())
    }
}

/// [`Keys`]/[`IntoKeys`] for Iterators of [`Key`]
#[derive(Debug, Clone, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct KeysIter<T: ?Sized>(T);

impl<T> Keys for KeysIter<T>
where
    T: Iterator + ?Sized,
    T::Item: Key,
{
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal> {
        let key = self.0.next().ok_or(Traversal::TooShort(0))?;
        key.find::<M>().ok_or(Traversal::NotFound(1))
    }

    fn finalize(&mut self) -> bool {
        self.0.next().is_none()
    }
}

impl<T> Keys for &mut T
where
    T: Keys + ?Sized,
{
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal> {
        T::next::<M>(self)
    }

    fn finalize(&mut self) -> bool {
        T::finalize(self)
    }
}

/// Be converted into a `Keys`
pub trait IntoKeys {
    /// The specific [`Keys`] implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a [`Keys`] implementor.
    fn into_keys(self) -> Self::IntoKeys;
}

impl<T> IntoKeys for T
where
    T: IntoIterator,
    <T::IntoIter as Iterator>::Item: Key,
{
    type IntoKeys = KeysIter<T::IntoIter>;

    fn into_keys(self) -> Self::IntoKeys {
        KeysIter(self.into_iter())
    }
}

/// Concatenate two `Keys` of different types
pub struct Chain<T, U>(T, U);

impl<T, U> Chain<T, U> {
    /// Return a new concatenated `Keys`
    pub fn new(t: T, u: U) -> Self {
        Self(t, u)
    }
}

impl<T: Keys, U: Keys> Keys for Chain<T, U> {
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal> {
        match self.0.next::<M>() {
            Err(Traversal::TooShort(_)) => self.1.next::<M>(),
            ret => ret,
        }
    }

    fn finalize(&mut self) -> bool {
        self.0.finalize() && self.1.finalize()
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
