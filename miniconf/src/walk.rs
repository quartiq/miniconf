use crate::{KeyLookup, Packed};

/// Metadata about a `TreeKey` namespace.
///
/// Metadata includes paths that may be [`crate::Traversal::Absent`] at runtime.
#[non_exhaustive]
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    pub count: usize,

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

    /// Return the walk starting point for an an empty internal node
    fn internal() -> Self;

    /// Return the walk starting point for a single leaf node
    fn leaf() -> Self;

    /// Merge node metadata into self.
    ///
    /// # Args
    /// * `walk`: The walk of the node to merge.
    /// * `index`: Either the child index or zero for homogeneous.
    /// * `lookup`: The namespace the node(s) are in.
    fn merge(self, walk: &Self, index: usize, lookup: &KeyLookup) -> Result<Self, Self::Error>;
}

impl Walk for Metadata {
    type Error = core::convert::Infallible;

    #[inline]
    fn internal() -> Self {
        Default::default()
    }

    #[inline]
    fn leaf() -> Self {
        Self {
            count: 1,
            ..Default::default()
        }
    }

    #[inline]
    fn merge(mut self, meta: &Self, index: usize, lookup: &KeyLookup) -> Result<Self, Self::Error> {
        let (len, count) = match lookup {
            KeyLookup::Named(names) => (names[index].len(), 1),
            KeyLookup::Numbered(_len) => {
                (index.checked_ilog10().unwrap_or_default() as usize + 1, 1)
            }
            KeyLookup::Homogeneous(len) => {
                debug_assert_eq!(index, 0);
                (len.ilog10() as usize + 1, len.get())
            }
        };
        self.max_depth = self.max_depth.max(1 + meta.max_depth);
        self.max_length = self.max_length.max(len + meta.max_length);
        debug_assert_ne!(meta.count, 0);
        self.count += count * meta.count;
        self.max_bits = self
            .max_bits
            .max(Packed::bits_for(lookup.len().get() - 1) + meta.max_bits);
        Ok(self)
    }
}
