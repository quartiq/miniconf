use core::{
    fmt::Write,
    ops::{Deref, DerefMut},
    slice::Iter,
};

use serde::{Deserialize, Serialize};

use crate::{Error, IntoKeys, KeysIter, Traversal, TreeKey};

/// Type of a node: leaf or internal
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum NodeType {
    /// A leaf node
    ///
    /// There are no nodes below this node
    Leaf,

    /// An internal node
    ///
    /// A non-leaf node with zero or more leaf nodes below this node
    Internal,
}

impl NodeType {
    /// Whether this node is a leaf
    #[inline]
    pub const fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf)
    }
}

/// Type and depth of a node in a `TreeKey`
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Node {
    /// The node depth
    ///
    /// This is the length of the key required to identify it.
    pub depth: usize,
    /// Leaf or internal
    pub typ: NodeType,
}

impl Node {
    /// The depth
    #[inline]
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// The node type
    #[inline]
    pub const fn typ(&self) -> NodeType {
        self.typ
    }

    /// The node is a leaf node
    #[inline]
    pub const fn is_leaf(&self) -> bool {
        self.typ.is_leaf()
    }

    /// Create a leaf node
    #[inline]
    pub const fn leaf(depth: usize) -> Self {
        Self {
            depth,
            typ: NodeType::Leaf,
        }
    }

    /// Create an inernal node
    #[inline]
    pub const fn internal(depth: usize) -> Self {
        Self {
            depth,
            typ: NodeType::Internal,
        }
    }
}

impl From<Node> for usize {
    fn from(value: Node) -> Self {
        value.depth
    }
}

/// Map a `TreeKey::traverse_by_key()` `Result` to a `Transcode::transcode()` `Result`.
impl TryFrom<Result<usize, Error<()>>> for Node {
    type Error = Traversal;
    fn try_from(value: Result<usize, Error<()>>) -> Result<Self, Traversal> {
        match value {
            Ok(depth) => Ok(Node::leaf(depth)),
            Err(Error::Traversal(Traversal::TooShort(depth))) => Ok(Node::internal(depth)),
            Err(Error::Inner(depth, ())) => Err(Traversal::TooShort(depth)),
            Err(Error::Traversal(err)) => Err(err),
            Err(Error::Finalization(())) => unreachable!(),
        }
    }
}

/// Look up an `IntoKeys` in a `TreeKey` and transcode it.
pub trait Transcode {
    /// Perform a node lookup of a `K: IntoKeys` on a `M: TreeKey<Y>` and transcode it.
    ///
    /// This modifies `self` such that afterwards `Self: IntoKeys` can be used on `M` again.
    /// It returns a `Node` with node type and depth information.
    ///
    /// Returning `Err(Traversal::TooShort(depth))` indicates that there was insufficient
    /// capacity and a key could not be encoded at the given depth.
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys;
}

/// Shim to provide the bare `Node` lookup without transcoding target
impl Transcode for () {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        M::traverse_by_key(keys.into_keys(), |_index, _name, _len| Ok::<_, ()>(())).try_into()
    }
}

/// Path with named keys separated by a separator char
///
/// The path will either be empty or start with the separator.
///
/// * `path: T`: A `Write` to write the separators and node names into during `Transcode`.
///   See also [TreeKey::traverse_all()] and `Metadata::max_length()` for upper bounds
///   on path length. Can also be a `AsRef<str>` to implement `IntoKeys` (see [`KeysIter`]).
/// * `const S: char`: The path hierarchy separator to be inserted before each name,
///   e.g. `'/'`.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Path<T: ?Sized, const S: char>(pub T);

impl<T: ?Sized, const S: char> Path<T, S> {
    /// The path hierarchy separator
    pub const fn separator(&self) -> char {
        S
    }
}

impl<T, const S: char> Path<T, S> {
    /// Extract just the path
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: ?Sized, const S: char> Deref for Path<T, S> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized, const S: char> DerefMut for Path<T, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T, const S: char> From<T> for Path<T, S> {
    fn from(value: T) -> Self {
        Path(value)
    }
}

/// String split/skip wrapper, smaller/simpler than `.split(S).skip(1)`
#[derive(Copy, Clone, Default, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[repr(transparent)]
pub struct PathIter<'a, const S: char>(Option<&'a str>);

impl<'a, const S: char> PathIter<'a, S> {
    /// Create a new `PathIter`
    #[inline]
    pub fn new(s: Option<&'a str>) -> Self {
        Self(s)
    }

    /// Create a new `PathIter` starting at the root.
    ///
    /// This calls `next()` once to pop everything up to and including the first separator.
    #[inline]
    pub fn root(s: &'a str) -> Self {
        let mut s = Self::new(Some(s));
        // Skip the first part to disambiguate between
        // the one-Key Keys `[""]` and the zero-Key Keys `[]`.
        // This is relevant in the case of e.g. `Option` and newtypes.
        // See the corresponding unittests (`just_option`).
        // It implies that Paths start with the separator
        // or are empty. Everything before the first separator is ignored.
        s.next();
        s
    }
}

impl<'a, const S: char> Iterator for PathIter<'a, S> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.0.map(|s| {
            let pos = s
                .chars()
                .map_while(|c| (c != S).then_some(c.len_utf8()))
                .sum();
            let (left, right) = s.split_at(pos);
            self.0 = right.get(S.len_utf8()..);
            left
        })
    }
}

impl<'a, const S: char> core::iter::FusedIterator for PathIter<'a, S> {}

impl<'a, T: AsRef<str> + ?Sized, const S: char> IntoKeys for &'a Path<T, S> {
    type IntoKeys = KeysIter<PathIter<'a, S>>;
    fn into_keys(self) -> Self::IntoKeys {
        PathIter::<'a, S>::root(self.0.as_ref()).into_keys()
    }
}

impl<T: Write + ?Sized, const S: char> Transcode for Path<T, S> {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        M::traverse_by_key(keys.into_keys(), |index, name, _len| {
            self.0.write_char(S).or(Err(()))?;
            let mut buf = itoa::Buffer::new();
            let name = name.unwrap_or_else(|| buf.format(index));
            debug_assert!(!name.contains(S));
            self.0.write_str(name).or(Err(()))
        })
        .try_into()
    }
}

/// Indices of `usize` to identify a node in a `TreeKey`
///
/// `T` can be `[usize; D]` for `Transcode` or `AsRef<[usize]>` for `IntoKeys` (see [`KeysIter`]).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Indices<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for Indices<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for Indices<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const D: usize> Default for Indices<[usize; D]> {
    fn default() -> Self {
        Self([0; D])
    }
}

impl<T> Indices<T> {
    /// Extract just the indices
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<const D: usize> From<Indices<[usize; D]>> for [usize; D] {
    fn from(value: Indices<[usize; D]>) -> Self {
        value.0
    }
}

impl<T> From<T> for Indices<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<'a, T: AsRef<[usize]> + ?Sized> IntoKeys for &'a Indices<T> {
    type IntoKeys = KeysIter<core::iter::Copied<Iter<'a, usize>>>;
    fn into_keys(self) -> Self::IntoKeys {
        self.0.as_ref().iter().copied().into_keys()
    }
}

impl<T: AsMut<[usize]> + ?Sized> Transcode for Indices<T> {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        let mut it = self.0.as_mut().iter_mut();
        M::traverse_by_key(keys.into_keys(), |index, _name, _len| {
            it.next().ok_or(()).map(|idx| {
                *idx = index;
            })
        })
        .try_into()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn strsplit() {
        use heapless::Vec;
        for p in ["/d/1", "/a/bccc//d/e/", "", "/", "a/b", "a"] {
            let a: Vec<_, 10> = PathIter::<'_, '/'>::root(p).collect();
            let b: Vec<_, 10> = p.split('/').skip(1).collect();
            assert_eq!(a, b);
        }
    }
}
