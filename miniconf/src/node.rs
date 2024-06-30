use core::{
    fmt::Write,
    iter::Skip,
    ops::{Deref, DerefMut},
    str::Split,
};

use slice_string::SliceString;

use crate::{Error, IntoKeys, KeysIter, Packed, Traversal, TreeKey};

/// Type of a node
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
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
pub struct Node {
    depth: usize,
    typ: NodeType,
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
    pub fn is_leaf(&self) -> bool {
        self.typ.is_leaf()
    }
}

/// Look up an IntoKeys on a TreeKey and return a node of different Keys with node type info
pub trait NodeLookup {
    /// Perform the lookup
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys;
}

fn traverse(ret: Result<usize, Error<()>>) -> Result<Node, Traversal> {
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

impl NodeLookup for () {
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
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

impl<'a> NodeLookup for SliceString<'a> {
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        let separator = self.chars().next().ok_or(Traversal::TooShort(0))?;
        self.clear();
        traverse(M::traverse_by_key(keys.into_keys(), |index, name, _len| {
            self.write_char(separator).or(Err(()))?;
            self.write_str(name.unwrap_or(itoa::Buffer::new().format(index)))
                .or(Err(()))
        }))
    }
}

/// Path separated by a separator
///
/// The path will either be empty or start with the separator.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Path<'a, T> {
    path: T,
    separator: &'a str,
}

impl<'a, T> Path<'a, T> {
    /// Create a new empty path
    pub fn new(path: T, separator: &'a str) -> Self {
        Self { path, separator }
    }

    /// Create a new empty path given a separator
    pub fn empty(separator: &'a str) -> Self
    where
        T: Default,
    {
        Self::new(T::default(), separator)
    }

    /// The path hierarchy separator
    pub fn separator(&self) -> &str {
        self.separator
    }

    /// The path
    pub fn path(&self) -> &T {
        &self.path
    }

    /// Extract just the path
    pub fn into_path(self) -> T {
        self.path
    }
}

impl<'a, T: AsRef<str>> IntoKeys for &'a Path<'a, T> {
    type IntoKeys = KeysIter<Skip<Split<'a, &'a str>>>;
    fn into_keys(self) -> Self::IntoKeys {
        self.path.as_ref().split(self.separator).skip(1).into_keys()
    }
}

impl<'a, T: Write> NodeLookup for Path<'a, T> {
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        traverse(M::traverse_by_key(keys.into_keys(), |index, name, _len| {
            self.path.write_str(self.separator).or(Err(()))?;
            self.path
                .write_str(name.unwrap_or(itoa::Buffer::new().format(index)))
                .or(Err(()))
        }))
    }
}

/// Slash-separated [`Path`]
#[derive(Debug, Clone, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct SlashPath<T>(Path<'static, T>);

impl<T> SlashPath<T> {
    /// Extract just the path
    pub fn into_path(self) -> T {
        self.0.into_path()
    }
}

impl<T: Default> Default for SlashPath<T> {
    fn default() -> Self {
        Self(Path::new(T::default(), "/"))
    }
}

impl<T> From<SlashPath<T>> for Path<'static, T> {
    fn from(value: SlashPath<T>) -> Self {
        value.0
    }
}

impl<T> From<T> for SlashPath<T> {
    fn from(value: T) -> Self {
        Self(Path::new(value, "/"))
    }
}

impl<T> Deref for SlashPath<T> {
    type Target = Path<'static, T>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for SlashPath<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a, T: AsRef<str>> IntoKeys for &'a SlashPath<T> {
    type IntoKeys = KeysIter<Skip<Split<'a, &'a str>>>;
    fn into_keys(self) -> Self::IntoKeys {
        self.0.into_keys()
    }
}

impl<T: Write> NodeLookup for SlashPath<T> {
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        self.0.lookup::<M, Y, _>(keys)
    }
}

impl<const D: usize> NodeLookup for [usize; D] {
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        let mut it = self.iter_mut();
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

impl NodeLookup for Packed {
    fn lookup<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        traverse(M::traverse_by_key(
            keys.into_keys(),
            |index, _name, len: usize| match self
                .push_lsb(Packed::bits_for(len.saturating_sub(1)), index)
            {
                None => Err(()),
                Some(_) => Ok(()),
            },
        ))
    }
}
