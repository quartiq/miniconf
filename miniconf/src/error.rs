/// Errors that can occur when using the Tree traits.
///
/// A `usize` member indicates the key depth where the error occurred.
/// The depth here is the number of names or indices consumed.
/// It is also the number of separators in a path or the length
/// of an indices slice.
///
/// If multiple errors are applicable simultaneously the precedence
/// is as per the order in the enum definition (from high to low).
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Traversal {
    /// A node does not exist at runtime.
    ///
    /// An `enum` variant in the tree towards the node is currently absent.
    /// This is for example the case if an [`Option`] using the `Tree*`
    /// traits is `None` at runtime. See also [`crate::TreeKey#option`].
    #[error("Variant absent (depth: {0})")]
    Absent(usize),

    /// The key ends early and does not reach a leaf node.
    #[error("Key does not reach a leaf (depth: {0})")]
    TooShort(usize),

    /// The key was not found (index parse failure or too large,
    /// name not found or invalid).
    #[error("Key not found (depth: {0})")]
    NotFound(usize),

    /// The key is too long and goes beyond a leaf node.
    #[error("Key goes beyond leaf (depth: {0})")]
    TooLong(usize),

    /// A node could not be accessed or is invalid.
    ///
    /// This is returned from custom implementations.
    #[error("Access/validation failure (depth: {0}): {1}")]
    Access(usize, &'static str),
}

impl Traversal {
    /// Pass it up one hierarchy depth level, incrementing its usize depth field by one.
    pub fn increment(self) -> Self {
        match self {
            Self::Absent(i) => Self::Absent(i + 1),
            Self::TooShort(i) => Self::TooShort(i + 1),
            Self::NotFound(i) => Self::NotFound(i + 1),
            Self::TooLong(i) => Self::TooLong(i + 1),
            Self::Access(i, msg) => Self::Access(i + 1, msg),
        }
    }

    /// Return the traversal depth
    #[inline]
    pub fn depth(&self) -> usize {
        match self {
            Self::Absent(i)
            | Self::TooShort(i)
            | Self::NotFound(i)
            | Self::TooLong(i)
            | Self::Access(i, _) => *i,
        }
    }
}

/// Compound errors
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error<E> {
    /// Tree traversal error
    #[error(transparent)]
    Traversal(#[from] Traversal),

    /// The value provided could not be serialized or deserialized
    /// or the traversal callback returned an error.
    #[error("(De)serialization (depth: {0}): {1}")]
    Inner(usize, #[source] E),

    /// There was an error during finalization.
    ///
    /// This is not to be returned by a TreeSerialize/TreeDeserialize
    /// implementation but only from a wrapper that creates and finalizes the
    /// the serializer/deserializer.
    ///
    /// The `Deserializer` has encountered an error only after successfully
    /// deserializing a value. This is the case if there is additional unexpected data.
    /// The `deserialize_by_key()` update takes place but this
    /// error will be returned.
    ///
    /// A `Serializer` may write checksums or additional framing data and fail with
    /// this error during finalization after the value has been serialized.
    #[error("(De)serializer finalization: {0}")]
    Finalization(#[source] E),
}

// Try to extract the Traversal from an Error
impl<E> TryFrom<Error<E>> for Traversal {
    type Error = Error<E>;
    #[inline]
    fn try_from(value: Error<E>) -> Result<Self, Self::Error> {
        match value {
            Error::Traversal(e) => Ok(e),
            e => Err(e),
        }
    }
}

impl<E> Error<E> {
    /// Pass an `Error<E>` up one hierarchy depth level, incrementing its usize depth field by one.
    #[inline]
    pub fn increment(self) -> Self {
        match self {
            Self::Traversal(t) => Self::Traversal(t.increment()),
            Self::Inner(i, e) => Self::Inner(i + 1, e),
            Self::Finalization(e) => Self::Finalization(e),
        }
    }
}
