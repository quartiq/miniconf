use core::num::NonZero;

use crate::{KeyLookup, Packed};

/// Capability to be created from a walk through all representative nodes in a
/// `TreeKey` using `traverse_all()`.
///
/// This is a bottom-up, breadth-first walk.
pub trait Walk: Sized {
    /// Create a leaf node
    fn leaf() -> Self;

    /// Create an internal node frmo child nodes.
    ///
    /// # Args
    /// * `children`: Child nodes to merge.
    /// * `lookup`: The namespace the child nodes are in.
    fn internal(children: &[Self], lookup: &KeyLookup) -> Self;
}

/// Metadata about a `TreeKey` namespace.
///
/// Metadata includes paths that may be [`crate::Traversal::Absent`] at runtime.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Metadata {
    /// The maximum length of a path in bytes.
    ///
    /// This is the exact maximum of the length of the concatenation of the node names
    /// in a [`crate::Path`] excluding the separators. See [`Self::max_length()`] for
    /// the maximum length including separators.
    pub max_length: usize,

    /// The maximum key depth.
    ///
    /// This is equal to the exact maximum number of path hierarchy separators.
    /// It's the exact maximum number of key indices.
    pub max_depth: usize,

    /// The exact total number of keys.
    pub count: NonZero<usize>,

    /// The maximum number of bits (see [`crate::Packed`])
    pub max_bits: u32,
}

impl Metadata {
    /// Add separator length to the maximum path length.
    ///
    /// To obtain an upper bound on the maximum length of all paths
    /// including separators, this adds `max_depth*separator_length`.
    #[inline]
    pub fn max_length(&self, separator: &str) -> usize {
        self.max_length + self.max_depth * separator.len()
    }
}

impl Walk for Metadata {
    #[inline]
    fn leaf() -> Self {
        Self {
            count: NonZero::<usize>::MIN,
            max_length: 0,
            max_depth: 0,
            max_bits: 0,
        }
    }

    #[inline]
    fn internal(children: &[Self], lookup: &KeyLookup) -> Self {
        let mut max_depth = 0;
        let mut max_length = 0;
        let mut count = 0;
        let mut max_bits = 0;
        // TODO: swap loop and match
        for (index, child) in children.iter().enumerate() {
            let (len, n) = match lookup {
                KeyLookup::Named(names) => {
                    debug_assert_eq!(children.len(), names.len());
                    (names[index].len(), 1)
                }
                KeyLookup::Numbered(len) => {
                    debug_assert_eq!(children.len(), len.get());
                    (index.checked_ilog10().unwrap_or_default() as usize + 1, 1)
                }
                KeyLookup::Homogeneous(len) => {
                    debug_assert_eq!(children.len(), 1);
                    (len.ilog10() as usize + 1, len.get())
                }
            };
            max_depth = max_depth.max(1 + child.max_depth);
            max_length = max_length.max(len + child.max_length);
            count += n * child.count.get();
            max_bits = max_bits.max(Packed::bits_for(lookup.len().get() - 1) + child.max_bits);
        }
        Self {
            max_bits,
            max_depth,
            max_length,
            count: NonZero::new(count).unwrap(),
        }
    }
}
