use core::convert::Infallible;
use core::fmt::Display;

use miniconf::SerdeError;
use minimq::{Property, PubError, ResourceError, types::Utf8String};
use strum::IntoStaticStr;

use crate::Error;

pub(crate) fn set_path<'a>(topic: &'a str, prefix: &str) -> Option<&'a str> {
    topic.strip_prefix(prefix)?.strip_prefix("/set")
}

#[derive(Debug, Copy, Clone, PartialEq, IntoStaticStr)]
pub(crate) enum ResponseCode {
    Ok,
    Error,
}

impl From<ResponseCode> for Property<'static> {
    fn from(value: ResponseCode) -> Self {
        Property::UserProperty(Utf8String("code"), Utf8String(value.into()))
    }
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

pub(crate) enum ResponseBody<E = serde_json_core::de::Error> {
    Lookup(DepthError<Infallible>),
    LeafRequired { depth: usize },
    Set(DepthError<E>),
}

impl<E: Display> Display for ResponseBody<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Lookup(err) => Display::fmt(err, f),
            Self::LeafRequired { .. } => f.write_str("Path does not resolve to a leaf"),
            Self::Set(err) => Display::fmt(err, f),
        }
    }
}

pub(crate) fn simple_pub_error<P, E>(err: PubError<P, E>) -> Error<E> {
    match err {
        PubError::Session(err) => Error::Mqtt(err),
        PubError::Payload(_) => Error::Mqtt(ResourceError::BufferTooSmall.into()),
    }
}
