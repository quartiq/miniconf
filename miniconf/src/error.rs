use core::fmt::{Debug, Display, Formatter};

/// Errors that can occur when using the Tree traits.
///
/// A `usize` member indicates the key depth where the error occurred.
/// The depth here is the number of names or indices consumed.
/// It is also the number of separators in a path or the length
/// of an indices slice.
///
/// If multiple errors are applicable simultaneously the precedence
/// is as per the order in the enum definition (from high to low).
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Traversal {
    /// A node does not exist at runtime.
    ///
    /// An `enum` variant in the tree towards the node is currently absent.
    /// This is for example the case if an [`Option`] using the `Tree*`
    /// traits is `None` at runtime. See also [`crate::TreeKey#option`].
    Absent(usize),

    /// The key ends early and does not reach a leaf node.
    TooShort(usize),

    /// The key was not found (index parse failure or too large,
    /// name not found or invalid).
    NotFound(usize),

    /// The key is too long and goes beyond a leaf node.
    TooLong(usize),

    /// A node could not be accessed.
    ///
    /// The `get` or `get_mut` accessor returned an error message.
    Access(usize, &'static str),

    /// A deserialized leaf value was found to be invalid.
    ///
    /// The `validate` callback returned an error message.
    Invalid(usize, &'static str),
}

impl Display for Traversal {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Traversal::Absent(depth) => {
                write!(f, "Variant absent (depth: {depth})")
            }
            Traversal::TooShort(depth) => {
                write!(f, "Key does not reach a leaf (depth: {depth})")
            }
            Traversal::NotFound(depth) => {
                write!(f, "Key not found (depth: {depth})")
            }
            Traversal::TooLong(depth) => {
                write!(f, "Key goes beyond a leaf (depth: {depth})")
            }
            Traversal::Access(depth, msg) => {
                write!(f, "Node accessor failed (depth: {depth}): {msg}")
            }
            Traversal::Invalid(depth, msg) => {
                write!(f, "Invalid deserialized value (depth: {depth}): {msg}")
            }
        }
    }
}

impl ::core::error::Error for Traversal {}

impl Traversal {
    /// Pass it up one hierarchy depth level, incrementing its usize depth field by one.
    #[inline]
    pub fn increment(self) -> Self {
        match self {
            Self::Absent(i) => Self::Absent(i + 1),
            Self::TooShort(i) => Self::TooShort(i + 1),
            Self::NotFound(i) => Self::NotFound(i + 1),
            Self::TooLong(i) => Self::TooLong(i + 1),
            Self::Access(i, msg) => Self::Access(i + 1, msg),
            Self::Invalid(i, msg) => Self::Invalid(i + 1, msg),
        }
    }

    /// Return the traversal depth
    #[inline]
    pub fn depth(&self) -> &usize {
        match self {
            Self::Absent(i)
            | Self::TooShort(i)
            | Self::NotFound(i)
            | Self::TooLong(i)
            | Self::Access(i, _)
            | Self::Invalid(i, _) => i,
        }
    }
}

/// Compound errors
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Error<E> {
    /// Tree traversal error
    Traversal(Traversal),

    /// The value provided could not be serialized or deserialized
    /// or the traversal callback returned an error.
    Inner(usize, E),

    /// There was an error during finalization.
    ///
    /// The `Deserializer` has encountered an error only after successfully
    /// deserializing a value. This is the case if there is additional unexpected data.
    /// The `deserialize_by_key()` update takes place but this
    /// error will be returned.
    ///
    /// A `Serializer` may write checksums or additional framing data and fail with
    /// this error during finalization after the value has been serialized.
    Finalization(E),
}

impl<E: Display> Display for Error<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Traversal(t) => {
                write!(f, "Traversal: {t}")
            }
            Self::Inner(depth, error) => {
                write!(f, "(De)serialization error (depth: {depth}): {error}")
            }
            Self::Finalization(error) => {
                write!(f, "(De)serializer finalization error: {error}")
            }
        }
    }
}

impl<E: core::error::Error + 'static> core::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        Some(match self {
            Self::Traversal(t) => t,
            Self::Inner(_, e) | Self::Finalization(e) => e,
        })
    }
}

// Try to extract the Traversal from an Error
impl<E> TryFrom<Error<E>> for Traversal {
    type Error = Error<E>;
    fn try_from(value: Error<E>) -> Result<Self, Self::Error> {
        match value {
            Error::Traversal(e) => Ok(e),
            e => Err(e),
        }
    }
}

impl<E> From<Traversal> for Error<E> {
    fn from(value: Traversal) -> Self {
        Self::Traversal(value)
    }
}

impl<E> Error<E> {
    /// Pass an `Error<E>` up one hierarchy depth level, incrementing its usize depth field by one.
    pub fn increment(self) -> Self {
        match self {
            Self::Traversal(t) => Self::Traversal(t.increment()),
            Self::Inner(i, e) => Self::Inner(i + 1, e),
            Self::Finalization(e) => Self::Finalization(e),
        }
    }

    /// Pass a `Result<usize, Error<E>>` up one hierarchy depth level, incrementing its usize depth field by one.
    pub fn increment_result(result: Result<usize, Self>) -> Result<usize, Self> {
        match result {
            Ok(i) => Ok(i + 1),
            Err(err) => Err(err.increment()),
        }
    }
}
