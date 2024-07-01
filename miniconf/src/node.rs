use core::{
    fmt::Write,
    iter::{Copied, Skip},
    ops::{Deref, DerefMut},
    slice::Iter,
    str::Split,
};

use serde::{Deserialize, Serialize};

use crate::{Error, IntoKeys, KeysIter, Traversal, TreeKey};

/// Type of a node
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
    pub const fn is_leaf(&self) -> bool {
        matches!(self, Self::Leaf)
    }
}

/// A node
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Node {
    /// The node key depth
    ///
    /// This is the length of the key required to identify it.
    pub depth: usize,
    /// Leaf or internal
    pub typ: NodeType,
}

impl Node {
    /// The depth
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// The NodeType
    pub const fn typ(&self) -> NodeType {
        self.typ
    }

    /// The node is a leaf node
    pub const fn is_leaf(&self) -> bool {
        self.typ.is_leaf()
    }

    /// Create a leaf node
    pub const fn leaf(depth: usize) -> Self {
        Self {
            depth,
            typ: NodeType::Leaf,
        }
    }

    /// Create an inernal node
    pub const fn internal(depth: usize) -> Self {
        Self {
            depth,
            typ: NodeType::Internal,
        }
    }
}

/// Look up an IntoKeys on a TreeKey and transcode the keys into self.
pub trait Transcode {
    /// Perform a lookup and transcode the keys into self
    ///
    /// Returns node type and depth info.
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys;
}

/// Map a `TreeKey::traverse_by_key()` `Result` to a `NodeLookup::lookup()` `Result`.
pub(crate) fn traverse(ret: Result<usize, Error<()>>) -> Result<Node, Traversal> {
    match ret {
        Ok(depth) => Ok(Node {
            depth,
            typ: NodeType::Leaf,
        }),
        Err(Error::Traversal(Traversal::TooShort(depth))) => Ok(Node {
            depth,
            typ: NodeType::Internal,
        }),
        Err(Error::Inner(depth, _err)) => Err(Traversal::TooShort(depth)),
        Err(Error::Traversal(err)) => Err(err),
        Err(Error::Finalization(_)) => unreachable!(),
    }
}

/// Shim to provide the bare `Node` lookup without transcoding the keys
impl Transcode for () {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        traverse(M::traverse_by_key(
            keys.into_keys(),
            |_index, _name, _len| Ok::<_, ()>(()),
        ))
    }
}

/// Path with named keys separated by a separator char
///
/// The path will either be empty or start with the separator.
///
/// * `path: T`: A `Write` to write the separators and node names into.
///   See also [TreeKey::metadata()] and `Metadata::max_length()` for upper bounds
///   on path length.
/// * `separator: const char`: The path hierarchy separator to be inserted before each name.
///
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Path<T, const S: char>(pub T);

impl<T, const S: char> Path<T, S> {
    /// The path hierarchy separator
    pub const fn separator(&self) -> char {
        S
    }

    /// Extract just the path
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T, const S: char> Deref for Path<T, S> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const S: char> DerefMut for Path<T, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T, const S: char> From<T> for Path<T, S> {
    fn from(value: T) -> Self {
        Path(value)
    }
}

impl<'a, T: AsRef<str>, const S: char> IntoKeys for &'a Path<T, S> {
    type IntoKeys = KeysIter<Skip<Split<'a, char>>>;
    fn into_keys(self) -> Self::IntoKeys {
        self.0.as_ref().split(self.separator()).skip(1).into_keys()
    }
}

impl<T: Write, const S: char> Transcode for Path<T, S> {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        traverse(M::traverse_by_key(keys.into_keys(), |index, name, _len| {
            self.0.write_char(self.separator()).or(Err(()))?;
            self.0
                .write_str(name.unwrap_or(itoa::Buffer::new().format(index)))
                .or(Err(()))
        }))
    }
}

/// Wrapper to have a Default impl for indices array
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Indices<T>(pub T);

impl<T> Deref for Indices<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Indices<T> {
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

impl<'a, T: AsRef<[usize]>> IntoKeys for &'a Indices<T> {
    type IntoKeys = KeysIter<Copied<Iter<'a, usize>>>;
    fn into_keys(self) -> Self::IntoKeys {
        self.0.as_ref().iter().copied().into_keys()
    }
}

impl<T: AsMut<[usize]>> Transcode for Indices<T> {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        let mut it = self.0.as_mut().iter_mut();
        traverse(M::traverse_by_key(
            keys.into_keys(),
            |index, _name, _len| {
                let idx = it.next().ok_or(())?;
                *idx = index;
                Ok::<_, ()>(())
            },
        ))
    }
}
