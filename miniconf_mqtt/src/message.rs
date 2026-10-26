use core::convert::Infallible;
use core::fmt::Display;

use miniconf::SerdeError;
use minimq::{PubError, ResourceError};

use crate::Error;

pub(crate) fn set_path<'a>(topic: &'a str, prefix: &str) -> Option<&'a str> {
    topic
        .strip_prefix(prefix)?
        .strip_prefix("/set")
        .filter(|path| path.is_empty() || path.starts_with('/'))
}

pub(crate) fn settings_path<'a>(topic: &'a str, prefix: &str) -> Option<&'a str> {
    topic
        .strip_prefix(prefix)?
        .strip_prefix("/settings")
        .filter(|path| path.is_empty() || path.starts_with('/'))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DepthError<E> {
    pub(crate) inner: SerdeError<E>,
    pub(crate) depth: usize,
}

impl<E> Display for DepthError<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} (depth {})", self.inner, self.depth)
    }
}

pub(crate) enum ResponseBody {
    Retained,
    Lookup(DepthError<Infallible>),
    LeafRequired { depth: usize },
    Set(DepthError<serde_json_core::de::Error>),
}

pub(crate) fn simple_pub_error<P, E>(err: PubError<P, E>) -> Error<E> {
    match err {
        PubError::Session(err) => Error::Mqtt(err),
        PubError::Payload(_) => Error::Mqtt(ResourceError::BufferTooSmall.into()),
    }
}
