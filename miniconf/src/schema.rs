//! Schema generation tools

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::num::NonZero;

use serde::Serialize;

use crate::{KeyLookup, Walk};

/// Internal/leaf node metadata
#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum Node<T> {
    /// A terminal leaf node
    Leaf(Option<T>),
    /// An internal node with named children
    Named(Vec<(&'static str, Node<T>)>),
    /// An internal node with numbered children of homogenenous type
    Homogeneous {
        /// Number of child nodes
        len: NonZero<usize>,
        /// Representative child node
        item: Box<Node<T>>,
    },
    /// An internal node with numbered children of heterogeneous type
    Numbered(Vec<Node<T>>),
}

impl<T: Clone> Walk for Node<T> {
    fn internal(children: &[Self], lookup: &KeyLookup) -> Self {
        match lookup {
            KeyLookup::Named(names) => Self::Named(
                names
                    .iter()
                    .copied()
                    .zip(children.iter().cloned())
                    .collect(),
            ),
            KeyLookup::Homogeneous(len) => Self::Homogeneous {
                len: *len,
                item: Box::new(children.first().unwrap().clone()),
            },
            KeyLookup::Numbered(_len) => Self::Numbered(children.to_vec()),
        }
    }

    fn leaf() -> Self {
        Self::Leaf(None)
    }
}

impl<T> Node<T> {
    /// Visit each node in the graph
    ///
    /// Pass the indices as well as the node by reference to the visitor
    ///
    /// Note that only the representative child will be visited for a
    /// homogeneous internal node.
    ///
    /// Top down, depth first.
    pub fn visit<F, E>(&self, root: &mut Vec<usize>, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&Vec<usize>, &Self) -> Result<(), E>,
    {
        func(root, self)?;
        match self {
            Self::Leaf(_) => {}
            Self::Homogeneous { item, .. } => {
                root.push(0); // at least one item guaranteed
                item.visit(root, func)?;
                root.pop();
            }
            Self::Named(map) => {
                for (i, (_, item)) in map.iter().enumerate() {
                    root.push(i);
                    item.visit(root, func)?;
                    root.pop();
                }
            }
            Self::Numbered(items) => {
                for (i, item) in items.iter().enumerate() {
                    root.push(i);
                    item.visit(root, func)?;
                    root.pop();
                }
            }
        }
        Ok(())
    }

    /// Visit each node in the graph mutably
    ///
    /// Pass the indices as well as the node by mutable reference to the visitor
    ///
    /// Note that only the representative child will be visited for a
    /// homogeneous internal node.
    ///
    /// top down, depth first.
    pub fn visit_mut<F, E>(&mut self, root: &mut Vec<usize>, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&Vec<usize>, &mut Self) -> Result<(), E>,
    {
        func(root, self)?;
        match self {
            Self::Leaf(_) => {}
            Self::Homogeneous { item, .. } => {
                root.push(0); // at least one item guaranteed
                item.visit_mut(root, func)?;
                root.pop();
            }
            Self::Named(map) => {
                for (i, (_, item)) in map.iter_mut().enumerate() {
                    root.push(i);
                    item.visit_mut(root, func)?;
                    root.pop();
                }
            }
            Self::Numbered(items) => {
                for (i, item) in items.iter_mut().enumerate() {
                    root.push(i);
                    item.visit_mut(root, func)?;
                    root.pop();
                }
            }
        }
        Ok(())
    }
}
