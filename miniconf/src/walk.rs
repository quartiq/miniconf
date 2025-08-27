use core::num::NonZero;

use crate::{Internal, Packed, Schema};

// TODO: Rename to Summary
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
    pub const fn max_length(&self, separator: &str) -> usize {
        self.max_length + self.max_depth * separator.len()
    }

    pub fn new(schema: &Schema) -> Self {
        // TODO: `const`
        if let Some(internal) = schema.internal.as_ref() {
            let mut max_depth = 0;
            let mut max_length = 0;
            let mut count = 0;
            let mut max_bits = 0;
            match internal {
                Internal::Named(nameds) => {
                    let bits = Packed::bits_for(nameds.len() - 1);
                    for named in nameds.iter() {
                        let child = Self::new(&named.schema);
                        max_depth = max_depth.max(1 + child.max_depth);
                        max_length = max_length.max(named.name.len() + child.max_length);
                        count += child.count.get();
                        max_bits = max_bits.max(bits + child.max_bits);
                    }
                }
                Internal::Numbered(numbereds) => {
                    let bits = Packed::bits_for(numbereds.len() - 1);
                    for (index, numbered) in numbereds.iter().enumerate() {
                        let len = index.checked_ilog10().unwrap_or_default() as usize + 1;
                        let child = Self::new(&numbered.schema);
                        max_depth = max_depth.max(1 + child.max_depth);
                        max_length = max_length.max(len + child.max_length);
                        count += child.count.get();
                        max_bits = max_bits.max(bits + child.max_bits);
                    }
                }
                Internal::Homogeneous(homogeneous) => {
                    let bits = Packed::bits_for(homogeneous.len.get() - 1);
                    let len = homogeneous.len.ilog10() as usize + 1;
                    let child = Self::new(&homogeneous.schema);
                    max_depth = max_depth.max(1 + child.max_depth);
                    max_length = max_length.max(len + child.max_length);
                    count += homogeneous.len.get() * child.count.get();
                    max_bits = max_bits.max(bits + child.max_bits);
                }
            }
            Self {
                max_bits,
                max_depth,
                max_length,
                count: NonZero::new(count).unwrap(),
            }
        } else {
            Self {
                count: NonZero::<usize>::MIN,
                max_length: 0,
                max_depth: 0,
                max_bits: 0,
            }
        }
    }
}
