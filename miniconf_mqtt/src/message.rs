use core::convert::Infallible;
use core::fmt::{Display, Write as _};

use heapless::{String, Vec};
use miniconf::SerdeError;
use minimq::{ProtocolError, Publication};
use serde_json_core::de;
use strum::IntoStaticStr;

use crate::{Error, MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH};

#[allow(clippy::large_enum_variant)]
pub(crate) enum Action {
    None(crate::State),
    Reply {
        state: crate::State,
        reply: Option<ReplyTarget>,
        code: ResponseCode,
        body: ReplyBody,
    },
    PublishSet {
        resource: Resource,
        reply: Option<ReplyTarget>,
        state: [usize; crate::MAX_DEPTH],
        depth: usize,
    },
    #[cfg(feature = "compat-settings-ingress")]
    OverrideSet {
        state: [usize; crate::MAX_DEPTH],
        depth: usize,
    },
}

pub(crate) enum ReplyBody {
    Lookup(DepthError<Infallible>),
    LeafRequired { depth: usize },
    Set(DepthError<de::Error>),
}

impl Display for ReplyBody {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Lookup(err) => Display::fmt(err, f),
            Self::LeafRequired { .. } => f.write_str("Path does not resolve to a leaf"),
            Self::Set(err) => Display::fmt(err, f),
        }
    }
}

#[derive(Copy, Clone)]
pub(crate) enum Resource {
    Set,
    #[cfg(feature = "compat-settings-ingress")]
    Settings,
}

impl Resource {
    pub(crate) fn parse<'a>(topic: &'a str, prefix: &str) -> Option<(Self, &'a str)> {
        let tail = topic.strip_prefix(prefix)?;
        #[cfg(feature = "compat-settings-ingress")]
        {
            [(Self::Settings, "/settings"), (Self::Set, "/set")]
                .into_iter()
                .find_map(|(resource, base)| tail.strip_prefix(base).map(|path| (resource, path)))
        }
        #[cfg(not(feature = "compat-settings-ingress"))]
        {
            tail.strip_prefix("/set").map(|path| (Self::Set, path))
        }
    }
}

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

pub(crate) fn format_slice<T: Display>(value: T, buf: &mut [u8]) -> Result<usize, ()> {
    struct FmtBuf<'a> {
        buf: &'a mut [u8],
        len: usize,
    }

    impl core::fmt::Write for FmtBuf<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let end = self.len.checked_add(bytes.len()).ok_or(core::fmt::Error)?;
            if end > self.buf.len() {
                return Err(core::fmt::Error);
            }
            self.buf[self.len..end].copy_from_slice(bytes);
            self.len = end;
            Ok(())
        }
    }

    let mut out = FmtBuf { buf, len: 0 };
    write!(&mut out, "{value}").map_err(|_| ())?;
    Ok(out.len)
}

pub(crate) fn simple_pub_error<P, E>(err: minimq::PubError<P, E>) -> Error<E> {
    match err {
        minimq::PubError::Session(err) => Error::Mqtt(err),
        minimq::PubError::Payload(_) => Error::Mqtt(ProtocolError::BufferSize.into()),
    }
}
