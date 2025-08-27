use core::{iter::Fuse, num::NonZero};
use serde::Serialize;

use crate::{Error, Metadata, Node, NodeIter, Packed, Transcode, Traversal};

macro_rules! max {
    ($a:expr, $b:expr) => {{
        let b = $b;
        if $a < b {
            $a = b;
        }
    }};
}

pub type Meta = Option<&'static [(&'static str, &'static str)]>;

/// Type of a node: leaf or internal
#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize, Default)]
pub struct Schema {
    /// Inner metadata
    pub meta: Meta,

    /// Internal node
    ///
    /// A non-leaf node with one or more leaf nodes below this node
    pub internal: Option<Internal>,
}

impl Schema {
    pub const LEAF: Self = Self::leaf(None);

    pub const fn leaf(meta: Meta) -> Self {
        Self {
            meta,
            internal: None,
        }
    }

    /// Whether this node is a leaf
    #[inline]
    pub const fn is_leaf(&self) -> bool {
        self.internal.is_none()
    }

    /// Number of child nodes
    #[inline]
    pub const fn len(&self) -> usize {
        match &self.internal {
            None => 0,
            Some(i) => i.len().get(),
        }
    }

    #[inline]
    pub fn next(&self, mut keys: impl Keys) -> Result<usize, Traversal> {
        keys.next(self.internal.as_ref().unwrap())
    }

    pub fn visit<F, E>(&self, idx: &mut [usize], depth: usize, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&[usize], &Schema) -> Result<(), E>,
    {
        func(&idx[..depth], self)?;
        if let Some(internal) = self.internal.as_ref() {
            if depth < idx.len() {
                match internal {
                    Internal::Homogeneous(h) => {
                        idx[depth] = 0; // at least one item guaranteed
                        h.schema.visit(idx, depth + 1, func)?;
                    }
                    Internal::Named(n) => {
                        for (i, n) in n.iter().enumerate() {
                            idx[depth] = i;
                            n.schema.visit(idx, depth + 1, func)?;
                        }
                    }
                    Internal::Numbered(n) => {
                        for (i, n) in n.iter().enumerate() {
                            idx[depth] = i;
                            n.schema.visit(idx, depth + 1, func)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn descend<F, E>(&self, mut func: F) -> Result<usize, Error<E>>
    where
        F: FnMut(&Self) -> Result<Option<&Self>, Error<E>>,
    {
        if let Some(child) = func(self)? {
            child
                .descend(func)
                .map_err(Error::increment)
                .map(|depth| depth + 1)
        } else {
            Ok(0)
        }
    }

    /// Traverse from the root to a leaf and call a function for each node.
    ///
    /// If a leaf is found early (`keys` being longer than required)
    /// `Err(Traversal(TooLong(depth)))` is returned.
    /// If `keys` is exhausted before reaching a leaf node,
    /// `Err(Traversal(TooShort(depth)))` is returned.
    /// `Traversal::Access/Invalid/Absent/Finalization` are never returned.
    ///
    /// This method should fail if and only if the key is invalid.
    /// It should succeed at least when any of the other key based methods
    /// in `TreeAny`, `TreeSerialize`, and `TreeDeserialize` succeed.
    ///
    /// ```
    /// use miniconf::{IntoKeys, Leaf, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    /// let mut ret = [(1, Some("bar"), 2), (0, None, 2)].into_iter();
    /// let func = |index, name, len: core::num::NonZero<usize>| -> Result<(), ()> {
    ///     assert_eq!(ret.next().unwrap(), (index, name, len.get()));
    ///     Ok(())
    /// };
    /// assert_eq!(S::traverse_by_key(["bar", "0"].into_keys(), func), Ok(2));
    /// ```
    ///
    /// # Args
    /// * `keys`: An `Iterator` of `Key`s identifying the node.
    /// * `func`: A `FnMut` to be called for each (internal and leaf) node on the path.
    ///   Its arguments are the index and the optional name of the node and the number
    ///   of top-level nodes at the given depth. Returning `Err(E)` aborts the traversal.
    ///   Returning `Ok(())` continues the downward traversal.
    ///
    /// # Returns
    /// Node depth on success (number of keys consumed/number of calls to `func`)
    ///
    /// # Design note
    /// Writing this to return an iterator instead of using a callback
    /// would have worse performance (O(n^2) instead of O(n) for matching)
    pub fn traverse<K, F, E>(&self, mut keys: K, mut func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(&Meta, Option<(usize, &Internal)>) -> Result<(), E>,
    {
        self.descend(|schema| {
            if let Some(internal) = schema.internal.as_ref() {
                let idx = keys.next(internal)?;
                func(&schema.meta, Some((idx, internal))).map_err(|err| Error::Inner(1, err))?;
                Ok(Some(internal.next(idx)))
            } else {
                func(&schema.meta, None).map_err(|err| Error::Inner(0, err))?;
                Ok(None)
            }
        })
    }

    /// Transcode keys to a new keys type representation
    ///
    /// The keys can be
    /// * too short: the internal node is returned
    /// * matched length: the leaf node is returned
    /// * too long: Err(TooLong(depth)) is returned
    ///
    /// In order to not require `N: Default`, use [`Transcode::transcode`] on
    /// an existing `&mut N`.
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 5],
    /// };
    ///
    /// let idx = [1, 1];
    ///
    /// let (path, node) = S::transcode::<Path<String, '/'>, _>(idx).unwrap();
    /// assert_eq!(path.as_str(), "/bar/1");
    /// let (path, node) = S::transcode::<JsonPath<String>, _>(idx).unwrap();
    /// assert_eq!(path.as_str(), ".bar[1]");
    /// let (indices, node) = S::transcode::<Indices<[_; 2]>, _>(&path).unwrap();
    /// assert_eq!(&indices[..node.depth()], idx);
    /// let (indices, node) = S::transcode::<Indices<[_; 2]>, _>(["bar", "1"]).unwrap();
    /// assert_eq!(&indices[..node.depth()], [1, 1]);
    /// let (packed, node) = S::transcode::<Packed, _>(["bar", "4"]).unwrap();
    /// assert_eq!(packed.into_lsb().get(), 0b1_1_100);
    /// let (path, node) = S::transcode::<Path<String, '/'>, _>(packed).unwrap();
    /// assert_eq!(path.as_str(), "/bar/4");
    /// let ((), node) = S::transcode(&path).unwrap();
    /// assert_eq!(node, Node::leaf(2));
    /// ```
    ///
    /// # Args
    /// * `keys`: `IntoKeys` to identify the node.
    ///
    /// # Returns
    /// Transcoded target and node information on success
    #[inline]
    pub fn transcode<N, K>(&self, keys: K) -> Result<(N, Node), Traversal>
    where
        K: IntoKeys,
        N: Transcode + Default,
    {
        let mut target = N::default();
        let node = target.transcode(self, keys)?;
        Ok((target, node))
    }

    /// Return an iterator over nodes of a given type
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [`TreeKey#option`]).
    /// An iterator with an exact and trusted `size_hint()` can be obtained from
    /// this through [`NodeIter::exact_size()`].
    /// The `D` const generic of [`NodeIter`] is the maximum key depth.
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeKey};
    /// #[derive(TreeKey)]
    /// struct S {
    ///     foo: Leaf<u32>,
    ///     bar: [Leaf<u16>; 2],
    /// };
    ///
    /// let paths: Vec<_> = S::nodes::<Path<String, '/'>, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect();
    /// assert_eq!(paths, ["/foo", "/bar/0", "/bar/1"]);
    ///
    /// let paths: Vec<_> = S::nodes::<JsonPath<String>, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_inner())
    ///     .collect();
    /// assert_eq!(paths, [".foo", ".bar[0]", ".bar[1]"]);
    ///
    /// let indices: Vec<_> = S::nodes::<Indices<[_; 2]>, 2>()
    ///     .exact_size()
    ///     .map(|p| {
    ///         let (idx, node) = p.unwrap();
    ///         (idx.into_inner(), node.depth)
    ///     })
    ///     .collect();
    /// assert_eq!(indices, [([0, 0], 1), ([1, 0], 2), ([1, 1], 2)]);
    ///
    /// let packed: Vec<_> = S::nodes::<Packed, 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().0.into_lsb().get())
    ///     .collect();
    /// assert_eq!(packed, [0b1_0, 0b1_1_0, 0b1_1_1]);
    ///
    /// let nodes: Vec<_> = S::nodes::<(), 2>()
    ///     .exact_size()
    ///     .map(|p| p.unwrap().1)
    ///     .collect();
    /// assert_eq!(nodes, [Node::leaf(1), Node::leaf(2), Node::leaf(2)]);
    /// ```
    ///
    #[inline]
    pub fn nodes<N, const D: usize>(&'static self) -> NodeIter<N, D>
    where
        N: Transcode + Default,
    {
        NodeIter::new(self)
    }

    pub const fn metadata(&self) -> Metadata {
        let mut m = Metadata {
            max_depth: 0,
            max_length: 0,
            count: NonZero::<usize>::MIN,
            max_bits: 0,
        };
        if let Some(internal) = self.internal.as_ref() {
            match internal {
                Internal::Named(nameds) => {
                    let bits = Packed::bits_for(nameds.len() - 1);
                    let mut index = 0;
                    while index < nameds.len() {
                        let named = &nameds[index];
                        let child = named.schema.metadata();
                        max!(m.max_depth, 1 + child.max_depth);
                        max!(m.max_length, named.name.len() + child.max_length);
                        m.count = m.count.checked_add(child.count.get()).unwrap();
                        max!(m.max_bits, bits + child.max_bits);
                        index += 1;
                    }
                }
                Internal::Numbered(numbereds) => {
                    let bits = Packed::bits_for(numbereds.len() - 1);
                    let mut index = 0;
                    while index < numbereds.len() {
                        let numbered = &numbereds[index];
                        let len = 1 + match index.checked_ilog10() {
                            None => 0,
                            Some(len) => len as usize,
                        };
                        let child = numbered.schema.metadata();
                        max!(m.max_depth, 1 + child.max_depth);
                        max!(m.max_length, len + child.max_length);
                        m.count = m.count.checked_add(child.count.get()).unwrap();
                        max!(m.max_bits, bits + child.max_bits);
                        index += 1;
                    }
                }
                Internal::Homogeneous(homogeneous) => {
                    m = homogeneous.schema.metadata();
                    m.max_depth += 1;
                    m.max_bits += Packed::bits_for(homogeneous.len.get() - 1);
                    m.max_length += 1 + homogeneous.len.ilog10() as usize;
                    m.count = m.count.checked_mul(homogeneous.len).unwrap();
                }
            }
        }
        m
    }
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Numbered {
    pub schema: &'static Schema,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Named {
    pub name: &'static str,
    pub schema: &'static Schema,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Homogeneous {
    pub len: NonZero<usize>,
    pub schema: &'static Schema,
    pub meta: Meta,
}

/// Data to look up field names and convert to indices
///
/// This struct used together with [`crate::TreeKey`].
#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub enum Internal {
    /// Named children
    Named(&'static [Named]),
    /// Numbered heterogeneous children
    Numbered(&'static [Numbered]),
    /// Homogeneous numbered children
    Homogeneous(Homogeneous),
}

impl Internal {
    /// Return the number of elements in the lookup
    #[inline]
    pub const fn len(&self) -> NonZero<usize> {
        match self {
            Self::Named(n) => NonZero::new(n.len()).expect("Must have at least one child"),
            Self::Numbered(n) => NonZero::new(n.len()).expect("Must have at least one child"),
            Self::Homogeneous(h) => h.len,
        }
    }

    pub const fn next(&self, idx: usize) -> &Schema {
        match self {
            Self::Named(nameds) => nameds[idx].schema,
            Self::Numbered(numbereds) => numbereds[idx].schema,
            Self::Homogeneous(homogeneous) => homogeneous.schema,
        }
    }

    /// Perform a index-to-name lookup
    ///
    /// If this succeeds with None, it's a numbered or homogeneous internal node and the
    /// name is the formatted index.
    #[inline]
    pub const fn lookup(&self, index: usize) -> Result<Option<&str>, Traversal> {
        if index >= self.len().get() {
            Err(Traversal::NotFound(1))
        } else {
            if let Self::Named(n) = self {
                Ok(Some(n[index].name))
            } else {
                Ok(None)
            }
        }
    }
}

/// Convert a `&str` key into a node index on a `KeyLookup`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find(&self, internal: &Internal) -> Result<usize, Traversal>;
}

impl<T: Key> Key for &T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, internal: &Internal) -> Result<usize, Traversal> {
        (**self).find(internal)
    }
}

impl<T: Key> Key for &mut T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, internal: &Internal) -> Result<usize, Traversal> {
        (**self).find(internal)
    }
}

// index
macro_rules! impl_key_integer {
    ($($t:ty)+) => {$(
        impl Key for $t {
            #[inline]
            fn find(&self, internal: &Internal) -> Result<usize, Traversal> {
                (*self)
                    .try_into()
                    .ok()
                    .filter(|i| *i < internal.len().get())
                    .ok_or(Traversal::NotFound(1))
            }
        }
    )+};
}
impl_key_integer!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

// name
impl Key for str {
    #[inline]
    fn find(&self, internal: &Internal) -> Result<usize, Traversal> {
        match internal {
            Internal::Named(n) => n.iter().position(|n| n.name == self),
            Internal::Numbered(n) => self.parse().ok().filter(|i| *i < n.len()),
            Internal::Homogeneous(h, ..) => self.parse().ok().filter(|i| *i < h.len.get()),
        }
        .ok_or(Traversal::NotFound(1))
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`KeyLookup`] and convert to `usize` index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, internal: &Internal) -> Result<usize, Traversal>;

    /// Finalize the keys, ensure there are no more.
    ///
    /// This must be fused.
    fn finalize(&mut self) -> Result<(), Traversal>;

    /// Chain another `Keys` to this one.
    #[inline]
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self, U::IntoKeys>
    where
        Self: Sized,
    {
        Chain(self, other.into_keys())
    }
}

impl<T> Keys for &mut T
where
    T: Keys + ?Sized,
{
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, Traversal> {
        (**self).next(internal)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        (**self).finalize()
    }
}

/// [`Keys`]/[`IntoKeys`] for Iterators of [`Key`]
#[derive(Debug, Clone)]
#[repr(transparent)]
pub struct KeysIter<T>(Fuse<T>);

impl<T: Iterator> KeysIter<T> {
    #[inline]
    fn new(inner: T) -> Self {
        Self(inner.fuse())
    }
}

impl<T> Keys for KeysIter<T>
where
    T: Iterator,
    T::Item: Key,
{
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, Traversal> {
        let n = self.0.next();
        n.ok_or(Traversal::TooShort(0))?.find(internal)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        let n = self.0.next();
        n.is_none().then_some(()).ok_or(Traversal::TooLong(0))
    }
}

/// Be converted into a `Keys`
pub trait IntoKeys {
    /// The specific `Keys` implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a `Keys` implementor.
    fn into_keys(self) -> Self::IntoKeys;
}

impl<T> IntoKeys for T
where
    T: IntoIterator,
    <T::IntoIter as Iterator>::Item: Key,
{
    type IntoKeys = KeysIter<T::IntoIter>;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        KeysIter::new(self.into_iter())
    }
}

impl<T> IntoKeys for KeysIter<T>
where
    T: Iterator,
    T::Item: Key,
{
    type IntoKeys = KeysIter<T>;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

/// Concatenate two `Keys` of different types
pub struct Chain<T, U>(T, U);

impl<T, U> Chain<T, U> {
    /// Return a new concatenated `Keys`
    #[inline]
    pub fn new(t: T, u: U) -> Self {
        Self(t, u)
    }
}

impl<T: Keys, U: Keys> Keys for Chain<T, U> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, Traversal> {
        match self.0.next(internal) {
            Err(Traversal::TooShort(_)) => self.1.next(internal),
            ret => ret,
        }
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        self.0.finalize().and_then(|()| self.1.finalize())
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
