use core::fmt::Write;

use slice_string::SliceString;

use crate::{Error, IntoKeys, Packed, Traversal, TreeKey};

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
    fn lookup<M, T, const Y: usize>(&mut self, keys: T) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y>,
        T: IntoKeys;
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
    fn lookup<M, T, const Y: usize>(&mut self, keys: T) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y>,
        T: IntoKeys,
    {
        traverse(M::traverse_by_key(
            keys.into_keys(),
            |_index, _name: Option<_>, _len| Ok::<_, ()>(()),
        ))
    }
}

impl<'a> NodeLookup for SliceString<'a> {
    fn lookup<M, T, const Y: usize>(&mut self, keys: T) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y>,
        T: IntoKeys,
    {
        let separator = self.chars().next().ok_or(Traversal::TooShort(0))?;
        self.clear();
        traverse(M::traverse_by_key(
            keys.into_keys(),
            |index, name: Option<_>, _len| {
                self.write_char(separator).or(Err(()))?;
                self.write_str(name.unwrap_or(itoa::Buffer::new().format(index)))
                    .or(Err(()))
            },
        ))
    }
}

impl<const D: usize> NodeLookup for [usize; D] {
    fn lookup<M, K, const Y: usize>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y>,
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
    fn lookup<M, K, const Y: usize>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y>,
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
