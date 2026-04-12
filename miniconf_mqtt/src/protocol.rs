use core::fmt::{Display, Write as FmtWrite};

use heapless::String;
use miniconf::SerdeError;
use minimq::{OwnedResponseTarget, ProtocolError};
use strum::IntoStaticStr;

use crate::{Error, MAX_RESPONSE_LENGTH, MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH};

pub(crate) type ReplyTarget = OwnedResponseTarget<MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH>;

#[derive(Debug, Copy, Clone, PartialEq, IntoStaticStr)]
pub(crate) enum ResponseCode {
    Ok,
    Continue,
    Error,
}

impl From<ResponseCode> for minimq::Property<'static> {
    fn from(value: ResponseCode) -> Self {
        minimq::Property::UserProperty(
            minimq::types::Utf8String("code"),
            minimq::types::Utf8String(value.into()),
        )
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

pub(crate) fn format_message<T: Display>(value: T) -> String<MAX_RESPONSE_LENGTH> {
    let mut text = String::new();
    if write!(&mut text, "{value}").is_err() {
        text.clear();
        text.push_str("Response too long").ok();
    }
    text
}

pub(crate) fn simple_pub_error(err: minimq::PubError<()>) -> Error {
    match err {
        minimq::PubError::Error(err) => Error::Mqtt(err),
        minimq::PubError::Serialization(()) => Error::Mqtt(ProtocolError::BufferSize.into()),
    }
}
