use core::fmt::{Display, Write as FmtWrite};

use heapless::{String, Vec};
use miniconf::SerdeError;
use minimq::{ProtocolError, Publication};
use strum::IntoStaticStr;

use crate::{
    MAX_PAYLOAD_LENGTH, MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH, client::ClientError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplyTarget {
    topic: String<MAX_TOPIC_LENGTH>,
    correlation_data: Option<Vec<u8, RESPONSE_CORRELATION_LENGTH>>,
}

impl ReplyTarget {
    pub(crate) fn new(topic: &str, correlation_data: Option<&[u8]>) -> Result<Self, ProtocolError> {
        Ok(Self {
            topic: String::try_from(topic).map_err(|_| ProtocolError::BufferSize)?,
            correlation_data: correlation_data
                .map(Vec::try_from)
                .transpose()
                .map_err(|_| ProtocolError::BufferSize)?,
        })
    }

    pub(crate) fn publication<'a, P>(&'a self, payload: P) -> Publication<'a, P> {
        let mut publication = Publication::new(self.topic.as_str(), payload);
        if let Some(data) = self.correlation_data.as_deref() {
            publication = publication.correlate(data);
        }
        publication
    }
}

#[derive(Debug, Copy, Clone, PartialEq, IntoStaticStr)]
pub(crate) enum ResponseCode {
    Ok,
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

pub(crate) fn rev_property<const N: usize>(buf: &mut String<N>, rev: u32) -> minimq::Property<'_> {
    buf.clear();
    write!(buf, "{rev}").ok();
    minimq::Property::UserProperty(
        minimq::types::Utf8String("rev"),
        minimq::types::Utf8String(buf.as_str()),
    )
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

pub(crate) fn format_message<T: Display>(value: T) -> String<MAX_PAYLOAD_LENGTH> {
    let mut text = String::new();
    if write!(&mut text, "{value}").is_err() {
        text.clear();
        text.push_str("Response too long").ok();
    }
    text
}

pub(crate) fn simple_pub_error<P, E>(err: minimq::PubError<P, E>) -> ClientError<E> {
    match err {
        minimq::PubError::Session(err) => ClientError::Mqtt(err),
        minimq::PubError::Payload(_) => ClientError::Mqtt(ProtocolError::BufferSize.into()),
    }
}
