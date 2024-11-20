use core::num::NonZero;

use crate::{KeyLookup, Packed};

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

/// Capability to be walked through a `TreeKey` using `traverse_all()`.
pub trait Walk: Sized {
    /// Error type for `merge()`
    type Error;

    /// Return the walk starting point for a single leaf node
    fn leaf() -> Self;

    /// Merge node metadata into self.
    ///
    /// # Args
    /// * `children`: The walk of the children to merge.
    /// * `lookup`: The namespace the node(s) are in.
    fn internal(children: &[&Self], lookup: &KeyLookup) -> Result<Self, Self::Error>;
}

impl Walk for Metadata {
    type Error = core::convert::Infallible;

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
    fn internal(children: &[&Self], lookup: &KeyLookup) -> Result<Self, Self::Error> {
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
        Ok(Self {
            max_bits,
            max_depth,
            max_length,
            count: NonZero::new(count).unwrap(),
        })
    }
}
