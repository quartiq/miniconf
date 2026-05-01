use core::convert::Infallible;
use core::fmt::Write as _;

use heapless::{String, Vec, VecView};
use log::{debug, warn};
use miniconf::{DescendError, Indices, SerdeError, ValueError, json_core};
use minimq::{InboundPublish, Property, QoS, Session};

use crate::{
    Error,
    client::{ChangedKey, Handle, Miniconf, ReplyTarget, Response},
    message::{DepthError, ResponseBody, ResponseCode, set_path, simple_pub_error},
};

pub(crate) enum ResponsePhase {
    Publish {
        state: ChangedKey,
        reply: Option<ReplyTarget>,
    },
    ErrorReply {
        target: ReplyTarget,
        message: ReplyMessage,
    },
    ReplyOk {
        target: ReplyTarget,
    },
    ReplyPublishError {
        target: ReplyTarget,
        error: String<96>,
        payload: String<96>,
    },
    Done,
}

pub(crate) struct ReplyMessage {
    code: ResponseCode,
    kind: &'static str,
    class: &'static str,
    error: String<96>,
    depth: Option<usize>,
    payload: String<96>,
}

pub(crate) fn handle<'msg, Settings>(
    prefix: &str,
    settings: &mut Settings,
    inbound: InboundPublish<'msg>,
) -> Handle<'msg>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
{
    let Some(path) = set_path(inbound.topic(), prefix) else {
        return Handle::Unhandled(inbound);
    };

    let reply = match inbound
        .reply_owned::<{ crate::MAX_TOPIC_LENGTH }, { crate::RESPONSE_CORRELATION_LENGTH }>()
    {
        Ok(reply) => reply,
        Err(err) => {
            warn!(
                "Rejecting request with oversized reply target on {}: {err:?}",
                inbound.topic()
            );
            return Handle::Ignored;
        }
    };

    let mut state = [0; crate::MAX_DEPTH];
    let lookup = match Settings::SCHEMA.resolve_into(path, &mut state) {
        Ok(lookup) => lookup,
        Err(err) => {
            debug!(
                "Rejecting set request topic={} err={err:?}",
                inbound.topic()
            );
            let body = ResponseBody::Lookup(DepthError::<Infallible> {
                inner: match err.error {
                    DescendError::Key(err) => SerdeError::Value(ValueError::Key(err)),
                    DescendError::Inner(()) => {
                        SerdeError::Value(ValueError::Access("Insufficient state"))
                    }
                },
                depth: err.lookup.depth,
            });
            return Handle::Rejected {
                response: reply.map(|target| Response {
                    phase: ResponsePhase::ErrorReply {
                        target,
                        message: encode_body(&body),
                    },
                }),
            };
        }
    };

    if inbound.payload().is_empty() {
        debug!("Ignoring empty set payload topic={}", inbound.topic());
        return Handle::Ignored;
    }

    if !lookup.schema.is_leaf() {
        debug!("Rejecting non-leaf set request topic={}", inbound.topic());
        let body = ResponseBody::LeafRequired {
            depth: lookup.depth,
        };
        return Handle::Rejected {
            response: reply.map(|target| Response {
                phase: ResponsePhase::ErrorReply {
                    target,
                    message: encode_body(&body),
                },
            }),
        };
    }

    let full = &state[..lookup.depth];
    match Miniconf::<Settings>::with_leaf(full, |keys| {
        json_core::set_by_keys(settings, keys, inbound.payload())
    }) {
        Ok(_) => {
            let changed = Indices::new(state, lookup.depth);
            Handle::Accepted {
                changed,
                response: Response {
                    phase: ResponsePhase::Publish {
                        state: changed,
                        reply,
                    },
                },
            }
        }
        Err(err) => {
            let body = ResponseBody::Set(err);
            Handle::Rejected {
                response: reply.map(|target| Response {
                    phase: ResponsePhase::ErrorReply {
                        target,
                        message: encode_body(&body),
                    },
                }),
            }
        }
    }
}

impl ResponsePhase {
    pub(crate) async fn step<'a, Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<'a, Settings>,
        session: &mut Session<'a, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
        IO: minimq::Io,
    {
        loop {
            match self {
                Self::Publish { state, reply } => {
                    match mm2.publish_current(session, settings, state.as_ref()).await {
                        Ok(()) => {
                            if let Some(target) = reply.take() {
                                *self = Self::ReplyOk { target };
                                continue;
                            }
                            *self = Self::Done;
                            return Ok(true);
                        }
                        Err(Error::Mqtt(minimq::Error::NotReady))
                        | Err(Error::Mqtt(minimq::Error::Protocol(
                            minimq::ProtocolError::InflightMetadataExhausted,
                        ))) => {
                            return Ok(false);
                        }
                        Err(err) => {
                            if let Some(target) = reply.take() {
                                let (error, payload) = publish_error_text(&err);
                                *self = Self::ReplyPublishError {
                                    target,
                                    error,
                                    payload,
                                };
                                continue;
                            }
                            return Err(err);
                        }
                    }
                }
                Self::ErrorReply { target, message } => {
                    match reply_message(session, target, message).await {
                        Ok(()) => {
                            *self = Self::Done;
                            return Ok(true);
                        }
                        Err(Error::Mqtt(minimq::Error::NotReady))
                        | Err(Error::Mqtt(minimq::Error::Protocol(
                            minimq::ProtocolError::InflightMetadataExhausted,
                        ))) => return Ok(false),
                        Err(err) => return Err(err),
                    }
                }
                Self::ReplyOk { target } => {
                    match reply_text(session, target, ResponseCode::Ok, b"").await {
                        Ok(()) => {
                            *self = Self::Done;
                            return Ok(true);
                        }
                        Err(Error::Mqtt(minimq::Error::NotReady))
                        | Err(Error::Mqtt(minimq::Error::Protocol(
                            minimq::ProtocolError::InflightMetadataExhausted,
                        ))) => return Ok(false),
                        Err(err) => return Err(err),
                    }
                }
                Self::ReplyPublishError {
                    target,
                    error,
                    payload,
                } => match reply_publish_error(session, target, error, payload.as_bytes()).await {
                    Ok(()) => {
                        *self = Self::Done;
                        return Ok(true);
                    }
                    Err(Error::Mqtt(minimq::Error::NotReady))
                    | Err(Error::Mqtt(minimq::Error::Protocol(
                        minimq::ProtocolError::InflightMetadataExhausted,
                    ))) => return Ok(false),
                    Err(err) => return Err(err),
                },
                Self::Done => return Ok(true),
            }
        }
    }
}

fn encode_body(body: &ResponseBody) -> ReplyMessage {
    let mut error = String::new();
    let mut payload = String::new();
    let (kind, class, depth) = match body {
        ResponseBody::Lookup(err) => {
            write!(&mut error, "{:?}", err.inner).ok();
            write!(&mut payload, "{err}").ok();
            ("lookup", "SerdeError", Some(err.depth))
        }
        ResponseBody::LeafRequired { depth } => {
            write!(&mut error, "{:?}", miniconf::KeyError::TooShort).ok();
            payload.push_str("Path does not resolve to a leaf").ok();
            ("set", "KeyError", Some(*depth))
        }
        ResponseBody::Set(err) => {
            write!(&mut error, "{:?}", err.inner).ok();
            write!(&mut payload, "{err}").ok();
            ("set", "SerdeError", Some(err.depth))
        }
    };
    ReplyMessage {
        code: ResponseCode::Error,
        kind,
        class,
        error,
        depth,
        payload,
    }
}

fn publish_error_text<E: core::fmt::Debug>(err: &Error<E>) -> (String<96>, String<96>) {
    let mut error = String::new();
    write!(&mut error, "{err:?}").ok();
    let mut payload = String::new();
    write!(&mut payload, "{err}").ok();
    (error, payload)
}

fn error_props<'a>(
    code: ResponseCode,
    kind: &'static str,
    class: &'static str,
    error: &'a str,
    depth: Option<&'a str>,
) -> Vec<Property<'a>, 5> {
    let mut props = Vec::new();
    push_prop(&mut props, "code", code.into());
    push_prop(&mut props, "kind", kind);
    push_prop(&mut props, "class", class);
    push_prop(&mut props, "error", error);
    if let Some(depth) = depth {
        push_prop(&mut props, "depth", depth);
    }
    props
}

fn push_prop<'a>(props: &mut VecView<Property<'a>>, key: &'static str, value: &'a str) {
    props
        .push(Property::UserProperty(
            minimq::types::Utf8String(key),
            minimq::types::Utf8String(value),
        ))
        .ok();
}

async fn reply_message<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    message: &ReplyMessage,
) -> Result<(), Error<IO::Error>>
where
    IO: minimq::Io,
{
    let mut depth_text = String::<16>::new();
    let depth = message.depth.and_then(|value| {
        use core::fmt::Write as _;
        write!(&mut depth_text, "{value}").ok()?;
        Some(depth_text.as_str())
    });
    let props = error_props(
        message.code,
        message.kind,
        message.class,
        message.error.as_str(),
        depth,
    );
    reply_bytes(session, target, &props, message.payload.as_bytes()).await
}

async fn reply_publish_error<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    error: &str,
    payload: &[u8],
) -> Result<(), Error<IO::Error>>
where
    IO: minimq::Io,
{
    let props = error_props(ResponseCode::Error, "publish", "Error", error, None);
    reply_bytes(session, target, &props, payload).await
}

async fn reply_text<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    code: ResponseCode,
    text: &[u8],
) -> Result<(), Error<IO::Error>>
where
    IO: minimq::Io,
{
    let props = [code.into()];
    reply_bytes(session, target, &props, text).await
}

async fn reply_bytes<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    props: &[Property<'_>],
    payload: &[u8],
) -> Result<(), Error<IO::Error>>
where
    IO: minimq::Io,
{
    session
        .publish(
            target
                .publication(payload)
                .properties(props)
                .qos(QoS::AtLeastOnce),
        )
        .await
        .map_err(simple_pub_error)
}
