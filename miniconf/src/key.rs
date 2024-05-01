use crate::Traversal;

/// The capability to look up top level field names and convert to indices
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
/// assert_eq!(S::len(), 2);
/// assert_eq!(S::name_to_index("bar"), Some(1));
/// ```
pub trait KeyLookup {
    /// The number of top-level nodes.
    ///
    /// This is used by `impl Keys for Packed`.
    fn len() -> usize;

    /// Convert a top level node name to a node index.
    ///
    /// The details of the mapping and the `usize` index values
    /// are an implementation detail and only need to be stable at runtime.
    /// This is used by `impl Key for &str`.
    fn name_to_index(name: &str) -> Option<usize>;
}

/// Capability to convert a key into a node index for a given `M: TreeKey`
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

/// Capability to yield [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`KeyLookup`] and convert to `usize` index.
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal>;

    /// Return whether there are more keys.
    ///
    /// This may mutate and consume remaining keys.
    fn is_empty(&mut self) -> bool;
}

impl<T> Keys for T
where
    T: Iterator,
    T::Item: Key,
{
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal> {
        let key = Iterator::next(self).ok_or(Traversal::TooShort(0))?;
        key.find::<M>().ok_or(Traversal::NotFound(1))
    }

    fn is_empty(&mut self) -> bool {
        self.next().is_none()
    }
}

/// Capability to be converted into a [`Keys`]
pub trait IntoKeys {
    /// The specific [`Keys`] implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a [`Keys`] implementor.
    fn into_keys(self) -> Self::IntoKeys;
}

impl<T> IntoKeys for T
where
    T: IntoIterator,
    T::IntoIter: Keys,
{
    type IntoKeys = T::IntoIter;

    fn into_keys(self) -> Self::IntoKeys {
        self.into_iter()
    }
}
