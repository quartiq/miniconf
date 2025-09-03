use core::{convert::Infallible, iter::Fuse, num::NonZero, ops::ControlFlow};
use serde::Serialize;

use crate::{DescendError, KeyError, NodeIter, Packed, Shape, Transcode};

// const a = a.max(b)
macro_rules! assign_max {
    ($a:expr, $b:expr) => {{
        let b = $b;
        if $a < b {
            $a = b;
        }
    }};
}

pub type Meta = Option<&'static [(&'static str, &'static str)]>;

/// Type of a node: leaf or internal
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize, Default)]
pub struct Schema {
    /// Inner metadata
    pub meta: Meta,

    /// Internal schemata
    pub internal: Option<Internal>,
}

impl Schema {
    /// A leaf without metadata
    pub const LEAF: Self = Self {
        meta: None,
        internal: None,
    };

    pub const fn numbered(numbered: &'static [Numbered]) -> Self {
        Self {
            meta: None,
            internal: Some(Internal::Numbered(numbered)),
        }
    }

    pub const fn named(named: &'static [Named]) -> Self {
        Self {
            meta: None,
            internal: Some(Internal::Named(named)),
        }
    }

    pub const fn homogeneous(homogeneous: Homogeneous) -> Self {
        Self {
            meta: None,
            internal: Some(Internal::Homogeneous(homogeneous)),
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

    /// Look up the next item from keys and return a child index
    ///
    /// # Panics
    /// On a leaf Schema.
    #[inline]
    pub fn next(&self, mut keys: impl Keys) -> Result<usize, KeyError> {
        keys.next(self.internal.as_ref().unwrap())
    }

    /// Visit all schemata with their indices
    pub fn visit<'a, E>(
        &'a self,
        idx: &mut [usize],
        depth: usize,
        outer: Option<&'a Self>,
        func: &mut impl FnMut(&[usize], Option<&'a Self>, &'a Self) -> Result<ControlFlow<()>, E>,
    ) -> Result<ControlFlow<()>, E> {
        if let Some(internal) = self.internal.as_ref() {
            if depth < idx.len() {
                match internal {
                    Internal::Homogeneous(h) => {
                        for i in 0..h.len.get() {
                            idx[depth] = i;
                            if h.schema.visit(idx, depth + 1, Some(self), func)?.is_break() {
                                break;
                            }
                        }
                    }
                    Internal::Named(n) => {
                        for (i, n) in n.iter().enumerate() {
                            idx[depth] = i;
                            if n.schema.visit(idx, depth + 1, Some(self), func)?.is_break() {
                                break;
                            }
                        }
                    }
                    Internal::Numbered(n) => {
                        for (i, n) in n.iter().enumerate() {
                            idx[depth] = i;
                            if n.schema.visit(idx, depth + 1, Some(self), func)?.is_break() {
                                break;
                            }
                        }
                    }
                }
            }
        }
        func(&idx[..depth], outer, self)
    }

    /// Visit all representative maximum length indices
    ///
    /// The recursive representative version of NodeIter
    pub fn visit_leaves<'a, E>(
        &'a self,
        idx: &mut [usize],
        mut func: impl FnMut(&[usize], &'a Self) -> Result<(), E>,
    ) -> Result<(), E> {
        let _ = self.visit(idx, 0, None, &mut |idx, outer, inner| {
            if inner.is_leaf() {
                func(idx, inner)?;
            }
            if outer
                .and_then(|o| o.internal.as_ref())
                .map(|i| matches!(i, Internal::Homogeneous(_)))
                .unwrap_or_default()
            {
                Ok(ControlFlow::Break(()))
            } else {
                Ok(ControlFlow::Continue(()))
            }
        })?;
        Ok(())
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
    /// use miniconf::{IntoKeys, Leaf, TreeSchema};
    /// #[derive(TreeSchema)]
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
    pub fn descend<'a, E>(
        &'a self,
        mut keys: impl Keys,
        mut func: impl FnMut(&'a Meta, Option<(usize, &'a Internal)>) -> Result<(), E>,
    ) -> Result<(), DescendError<E>> {
        let mut schema = self;
        while let Some(internal) = schema.internal.as_ref() {
            let idx = keys.next(internal)?;
            func(&schema.meta, Some((idx, internal))).map_err(DescendError::Inner)?;
            schema = internal.get_schema(idx);
        }
        keys.finalize()?;
        func(&schema.meta, None).map_err(DescendError::Inner)
    }

    /// Look up outer and inner metadata given keys.
    pub fn get_meta(&self, keys: impl IntoKeys) -> Result<(Option<&Meta>, &Meta), KeyError> {
        let mut outer = None;
        let mut inner = &self.meta;
        self.descend(keys.into_keys(), |meta, idx_internal| {
            if let Some((idx, internal)) = idx_internal {
                outer = Some(internal.get_meta(idx));
            }
            inner = meta;
            Ok::<_, Infallible>(())
        })
        .map_err(|e| e.try_into().unwrap())?;
        Ok((outer, inner))
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
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeSchema};
    /// #[derive(TreeSchema)]
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
    pub fn transcode<N: Transcode + Default>(
        &self,
        keys: impl IntoKeys,
    ) -> Result<N, DescendError<N::Error>> {
        let mut target = N::default();
        target.transcode(self, keys)?;
        Ok(target)
    }

    /// Return an iterator over nodes of a given type
    ///
    /// This is a walk of all leaf nodes.
    /// The iterator will walk all paths, including those that may be absent at
    /// runtime (see [`TreeSchema#option`]).
    /// An iterator with an exact and trusted `size_hint()` can be obtained from
    /// this through [`NodeIter::exact_size()`].
    /// The `D` const generic of [`NodeIter`] is the maximum key depth.
    ///
    /// ```
    /// use miniconf::{Indices, JsonPath, Leaf, Node, Packed, Path, TreeSchema};
    /// #[derive(TreeSchema)]
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

    /// Compute metadata
    pub const fn shape(&self) -> Shape {
        let mut m = Shape {
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
                    let mut count = 0;
                    while index < nameds.len() {
                        let named = &nameds[index];
                        let child = named.schema.shape();
                        assign_max!(m.max_depth, 1 + child.max_depth);
                        assign_max!(m.max_length, named.name.len() + child.max_length);
                        assign_max!(m.max_bits, bits + child.max_bits);
                        count += child.count.get();
                        index += 1;
                    }
                    m.count = NonZero::new(count).unwrap();
                }
                Internal::Numbered(numbereds) => {
                    let bits = Packed::bits_for(numbereds.len() - 1);
                    let mut index = 0;
                    let mut count = 0;
                    while index < numbereds.len() {
                        let numbered = &numbereds[index];
                        let len = 1 + match index.checked_ilog10() {
                            None => 0,
                            Some(len) => len as usize,
                        };
                        let child = numbered.schema.shape();
                        assign_max!(m.max_depth, 1 + child.max_depth);
                        assign_max!(m.max_length, len + child.max_length);
                        assign_max!(m.max_bits, bits + child.max_bits);
                        count += child.count.get();
                        index += 1;
                    }
                    m.count = NonZero::new(count).unwrap();
                }
                Internal::Homogeneous(homogeneous) => {
                    m = homogeneous.schema.shape();
                    m.max_depth += 1;
                    m.max_length += 1 + homogeneous.len.ilog10() as usize;
                    m.max_bits += Packed::bits_for(homogeneous.len.get() - 1);
                    m.count = m.count.checked_mul(homogeneous.len).unwrap();
                }
            }
        }
        m
    }
}

/// A numbered schema item
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub struct Numbered {
    pub schema: &'static Schema,
    pub meta: Meta,
}

impl Numbered {
    pub const fn new(schema: &'static Schema) -> Self {
        Self { meta: None, schema }
    }
}

/// A named schema item
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub struct Named {
    pub name: &'static str,
    pub schema: &'static Schema,
    pub meta: Meta,
}

impl Named {
    pub const fn new(name: &'static str, schema: &'static Schema) -> Self {
        Self {
            meta: None,
            name,
            schema,
        }
    }
}

/// A representative schema item for a homogeneous array
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub struct Homogeneous {
    pub len: NonZero<usize>,
    pub schema: &'static Schema,
    pub meta: Meta,
}

impl Homogeneous {
    pub const fn new(len: usize, schema: &'static Schema) -> Self {
        let len = match NonZero::new(len) {
            Some(len) => len,
            None => panic!("Must have at least one child"),
        };
        Self {
            meta: None,
            len,
            schema,
        }
    }
}

/// An internal node with children
///
/// Always non-empty
#[derive(Clone, Debug, PartialEq, PartialOrd, Eq, Ord, Hash, Serialize)]
pub enum Internal {
    /// Named children
    Named(&'static [Named]),
    /// Numbered heterogeneous children
    Numbered(&'static [Numbered]),
    /// Homogeneous numbered children
    Homogeneous(Homogeneous),
}

impl Internal {
    /// Return the number of direct child nodes
    #[inline]
    pub const fn len(&self) -> NonZero<usize> {
        match self {
            Self::Named(n) => NonZero::new(n.len()).expect("Must have at least one child"),
            Self::Numbered(n) => NonZero::new(n.len()).expect("Must have at least one child"),
            Self::Homogeneous(h) => h.len,
        }
    }

    /// Return the child schema at the given index
    ///
    /// # Panics
    /// If the index is out of bounds
    pub const fn get_schema(&self, idx: usize) -> &Schema {
        match self {
            Self::Named(nameds) => nameds[idx].schema,
            Self::Numbered(numbereds) => numbereds[idx].schema,
            Self::Homogeneous(homogeneous) => homogeneous.schema,
        }
    }

    /// Return the outer metadata for the given child
    ///
    /// # Panics
    /// If the index is out of bounds
    pub const fn get_meta(&self, idx: usize) -> &Meta {
        match self {
            Internal::Named(nameds) => &nameds[idx].meta,
            Internal::Numbered(numbereds) => &numbereds[idx].meta,
            Internal::Homogeneous(homogeneous) => &homogeneous.meta,
        }
    }

    /// Perform a index-to-name lookup
    ///
    /// If this succeeds with None, it's a numbered or homogeneous internal node and the
    /// name is the formatted index.
    ///
    /// # Panics
    /// If the index is out of bounds
    #[inline]
    pub const fn get_name(&self, idx: usize) -> Option<&str> {
        if let Self::Named(n) = self {
            Some(n[idx].name)
        } else {
            None
        }
    }

    /// Perform a name-to-index lookup
    #[inline]
    pub fn get_index(&self, name: &str) -> Option<usize> {
        match self {
            Internal::Named(n) => n.iter().position(|n| n.name == name),
            Internal::Numbered(n) => name.parse().ok().filter(|i| *i < n.len()),
            Internal::Homogeneous(h, ..) => name.parse().ok().filter(|i| *i < h.len.get()),
        }
    }
}

/// Convert a key into a node index given an internal node schema
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find(&self, internal: &Internal) -> Option<usize>;
}

impl<T: Key> Key for &T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, internal: &Internal) -> Option<usize> {
        (**self).find(internal)
    }
}

impl<T: Key> Key for &mut T
where
    T: Key + ?Sized,
{
    #[inline]
    fn find(&self, internal: &Internal) -> Option<usize> {
        (**self).find(internal)
    }
}

// index
macro_rules! impl_key_integer {
    ($($t:ty)+) => {$(
        impl Key for $t {
            #[inline]
            fn find(&self, internal: &Internal) -> Option<usize> {
                (*self).try_into().ok().filter(|i| *i < internal.len().get())
            }
        }
    )+};
}
impl_key_integer!(usize u8 u16 u32 u64 u128 isize i8 i16 i32 i64 i128);

// name
impl Key for str {
    #[inline]
    fn find(&self, internal: &Internal) -> Option<usize> {
        internal.get_index(self)
    }
}

/// Capability to yield and look up [`Key`]s
pub trait Keys: Sized {
    /// Look up the next key in a [`Internal`] and convert to `usize` index.
    ///
    /// This must be fused (like [`core::iter::FusedIterator`]).
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError>;

    /// Finalize the keys, ensure there are no more.
    ///
    /// This must be fused.
    fn finalize(&mut self) -> Result<(), KeyError>;

    /// Chain another `Keys` to this one.
    #[inline]
    fn chain<U: IntoKeys>(self, other: U) -> Chain<Self, U::IntoKeys> {
        Chain::new(self, other.into_keys())
    }

    /// Track consumption
    #[inline]
    fn track(self) -> Track<Self> {
        Track {
            inner: self,
            depth: 0,
        }
    }

    #[inline]
    fn short(self) -> Short<Self> {
        Short {
            inner: self,
            leaf: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Short<K> {
    pub inner: K,
    pub leaf: bool,
}

impl<K> Short<K> {
    pub fn new(inner: K) -> Self {
        Self { inner, leaf: false }
    }
}

impl<K: Keys> IntoKeys for &mut Short<K> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

impl<K: Keys> Keys for Short<K> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        self.inner.next(internal)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        let ret = self.inner.finalize();
        self.leaf = ret.is_ok();
        ret
    }
}

impl<T: Transcode> Transcode for Short<T> {
    type Error = T::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        self.leaf = false;
        match self.inner.transcode(schema, keys) {
            Err(DescendError::Key(KeyError::TooShort)) => Ok(()),
            Ok(()) | Err(DescendError::Key(KeyError::TooLong)) => {
                self.leaf = true;
                Ok(())
            }
            ret => ret,
        }
    }
}

/// Track keys consumption and leaf encounter
#[derive(Clone, Debug, Default, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Track<K> {
    pub inner: K,
    pub depth: usize,
}

impl<K> Track<K> {
    pub fn new(inner: K) -> Self {
        Self { inner, depth: 0 }
    }
}

impl<K: Keys> IntoKeys for &mut Track<K> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

impl<K: Keys> Keys for Track<K> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let k = self.inner.next(internal);
        if k.is_ok() {
            self.depth += 1;
        }
        k
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        self.inner.finalize()
    }
}

impl<T: Transcode> Transcode for Track<T> {
    type Error = T::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        let mut tracked = keys.into_keys().track();
        let ret = self.inner.transcode(schema, &mut tracked);
        self.depth = tracked.depth;
        ret
    }
}

impl<T> Keys for &mut T
where
    T: Keys + ?Sized,
{
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        (**self).next(internal)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
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
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let n = self.0.next().ok_or(KeyError::TooShort)?;
        n.find(internal).ok_or(KeyError::NotFound)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        match self.0.next() {
            Some(_) => Err(KeyError::TooLong),
            None => Ok(()),
        }
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
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        match self.0.next(internal) {
            Err(KeyError::TooShort) => self.1.next(internal),
            ret => ret,
        }
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), KeyError> {
        self.0.finalize().and_then(|_| self.1.finalize())
    }
}

impl<T: Keys, U: Keys> IntoKeys for Chain<T, U> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}
