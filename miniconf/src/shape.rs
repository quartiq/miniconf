use core::num::NonZero;

use crate::{Internal, Packed, Schema};

/// Metadata about a [`crate::TreeSchema`] namespace.
///
/// Metadata includes paths that may be [`crate::ValueError::Absent`] at runtime.
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Shape {
    /// The maximum length of a path in bytes.
    ///
    /// This is the exact maximum of the length of the concatenation of the node names
    /// in a [`crate::Path`] excluding the separators. See [`Self::max_length()`] for
    /// the maximum length including separators.
    pub max_length: usize,

    /// The maximum node depth.
    ///
    /// This is equal to the exact maximum number of path hierarchy separators.
    /// It's the exact maximum number of key indices.
    pub max_depth: usize,

    /// The exact total number of leaf nodes.
    pub count: NonZero<usize>,

    /// The maximum number of bits (see [`Packed`])
    pub max_bits: u32,
}

// const a = a.max(b)
macro_rules! assign_max {
    ($a:expr, $b:expr) => {{
        let b = $b;
        if $a < b {
            $a = b;
        }
    }};
}

impl Shape {
    /// Add separator length to the maximum path length.
    ///
    /// To obtain an upper bound on the maximum length of all paths
    /// including separators, this adds `max_depth*separator_length`.
    pub const fn max_length(&self, separator: &str) -> usize {
        self.max_length + self.max_depth * separator.len()
    }

    /// Recursively compute Shape for a Schema.
    pub const fn new(schema: &Schema) -> Self {
        let mut m = Self {
            max_depth: 0,
            max_length: 0,
            count: NonZero::<usize>::MIN,
            max_bits: 0,
        };
        if let Some(internal) = schema.internal() {
            let len = internal.len().get();
            let bits = Packed::bits_for(len - 1);
            match internal {
                Internal::Named(_) | Internal::Numbered(_) => {
                    let mut index = 0;
                    let mut count = 0;
                    while index < len {
                        let name_len = match internal.get_name(index) {
                            Some(name) => name.len(),
                            None => match index.checked_ilog10() {
                                None => 1,
                                Some(digits) => 1 + digits as usize,
                            },
                        };
                        let child = Self::new(internal.get_schema(index));
                        assign_max!(m.max_depth, 1 + child.max_depth);
                        assign_max!(m.max_length, name_len + child.max_length);
                        assign_max!(m.max_bits, bits + child.max_bits);
                        count += child.count.get();
                        index += 1;
                    }
                    m.count = NonZero::new(count).unwrap();
                }
                Internal::Homogeneous(homogeneous) => {
                    m = Self::new(homogeneous.schema);
                    m.max_depth += 1;
                    m.max_length += match (len - 1).checked_ilog10() {
                        Some(digits) => 1 + digits as usize,
                        None => 1,
                    };
                    m.max_bits += bits;
                    m.count = m.count.checked_mul(homogeneous.len).unwrap();
                }
            }
        }
        m
    }
}
