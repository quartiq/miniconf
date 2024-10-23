use crate::KeyLookup;

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
    /// * `index`: Either the node index in case of a single node
    ///   or `None`, in case of `lookup.len` nodes of homogeneous type.
    /// * `lookup`: The namespace the node(s) are in.
    fn merge(
        self,
        walk: &Self,
        index: Option<usize>,
        lookup: &KeyLookup,
    ) -> Result<Self, Self::Error>;
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

    fn merge(
        mut self,
        meta: &Self,
        index: Option<usize>,
        lookup: &KeyLookup,
    ) -> Result<Self, Self::Error> {
        let (ident_len, count) = match index {
            None => (
                match lookup.names {
                    Some(names) => names.iter().map(|n| n.len()).max().unwrap_or_default(),
                    None => lookup.len.checked_ilog10().unwrap_or_default() as usize + 1,
                },
                lookup.len,
            ),
            Some(index) => (
                match lookup.names {
                    Some(names) => names[index].len(),
                    None => index.checked_ilog10().unwrap_or_default() as usize + 1,
                },
                1,
            ),
        };
        self.max_depth = self.max_depth.max(1 + meta.max_depth);
        self.max_length = self.max_length.max(ident_len + meta.max_length);
        self.count += count * meta.count;
        Ok(self)
    }
}
