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
pub enum KeyError {
    /// The key ends early and does not reach a leaf node.
    #[error("Key does not reach a leaf")]
    TooShort,

    /// The key was not found (index parse failure or too large,
    /// name not found or invalid).
    #[error("Key not found")]
    NotFound,

    /// The key is too long and goes beyond a leaf node.
    #[error("Key goes beyond a leaf")]
    TooLong,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DescendError<E> {
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error("Visitor failed: {0}")]
    Inner(#[source] E),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValueError {
    /// Tree traversal error
    #[error(transparent)]
    Key(#[from] KeyError),

    /// A node does not exist at runtime.
    ///
    /// An `enum` variant in the tree towards the node is currently absent.
    /// This is for example the case if an [`Option`] using the `Tree*`
    /// traits is `None` at runtime. See also [`crate::TreeKey#option`].
    #[error("Variant absent")]
    Absent,

    /// A node could not be accessed or is invalid.
    ///
    /// This is returned from custom implementations.
    #[error("Access/validation failure: {0}")]
    Access(&'static str),
}

/// Compound errors
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SerDeError<E> {
    #[error(transparent)]
    Value(#[from] ValueError),

    /// The value provided could not be serialized or deserialized
    /// or the traversal callback returned an error.
    #[error("(De)serialization: {0}")]
    Inner(#[source] E),

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

impl<E> From<KeyError> for SerDeError<E> {
    #[inline]
    fn from(value: KeyError) -> Self {
        SerDeError::Value(value.into())
    }
}

// Try to extract the Traversal from an Error
impl<E> TryFrom<SerDeError<E>> for KeyError {
    type Error = SerDeError<E>;
    #[inline]
    fn try_from(value: SerDeError<E>) -> Result<Self, Self::Error> {
        match value {
            SerDeError::Value(ValueError::Key(e)) => Ok(e),
            e => Err(e),
        }
    }
}

// Try to extract the Traversal from an Error
impl TryFrom<ValueError> for KeyError {
    type Error = ValueError;
    #[inline]
    fn try_from(value: ValueError) -> Result<Self, Self::Error> {
        match value {
            ValueError::Key(e) => Ok(e),
            e => Err(e),
        }
    }
}

// Try to extract the Traversal from an Error
impl<E> TryFrom<DescendError<E>> for KeyError {
    type Error = E;
    #[inline]
    fn try_from(value: DescendError<E>) -> Result<Self, Self::Error> {
        match value {
            DescendError::Key(e) => Ok(e),
            DescendError::Inner(e) => Err(e),
        }
    }
}

// Try to extract the Traversal from an Error
impl<E> TryFrom<SerDeError<E>> for ValueError {
    type Error = E;
    #[inline]
    fn try_from(value: SerDeError<E>) -> Result<Self, Self::Error> {
        match value {
            SerDeError::Value(e) => Ok(e),
            SerDeError::Finalization(e) | SerDeError::Inner(e) => Err(e),
        }
    }
}
