use core::convert::Infallible;
use core::fmt::Write as _;

use heapless::{String, Vec, VecView};
use miniconf::{DescendError, Indices, SerdeError, ValueError, json_core};
use minimq::{
    Error as MqttError, InboundPublish, Io, Op, Property, QoS, ResourceError, Session,
    types::Utf8String,
};

use crate::{
    Error,
    client::{Aftermath, ChangedKey, Miniconf, PendingOp, ReplyTarget, Route},
    message::{DepthError, ResponseBody, ResponseCode, set_path, simple_pub_error},
};

pub(crate) enum AftermathPhase {
    Publish {
        state: ChangedKey,
        reply: Option<ReplyTarget>,
        op: Option<Op>,
    },
    ErrorReply {
        target: ReplyTarget,
        message: ReplyMessage,
        op: Option<Op>,
    },
    ReplyOk {
        target: ReplyTarget,
        op: Option<Op>,
    },
    ReplyPublishError {
        target: ReplyTarget,
        error: String<96>,
        payload: String<96>,
        op: Option<Op>,
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

pub(crate) fn is_request(prefix: &str, topic: &str) -> bool {
    set_path(topic, prefix).is_some()
}

pub(crate) fn route<'msg, Settings>(
    prefix: &str,
    settings: &mut Settings,
    inbound: InboundPublish<'msg>,
) -> Route<'msg>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
{
    let Some(path) = set_path(inbound.topic(), prefix) else {
        return Route::Unhandled(inbound);
    };

    let reply = match inbound
        .reply_owned::<{ crate::MAX_TOPIC_LENGTH }, { crate::RESPONSE_CORRELATION_LENGTH }>()
    {
        Ok(reply) => reply,
        Err(err) => {
            crate::warn!(
                "Rejecting request with oversized reply target topic={=str} err={}",
                inbound.topic(),
                err
            );
            return Route::Ignored;
        }
    };

    let mut state = [0; crate::MAX_DEPTH];
    let lookup = match Settings::SCHEMA.resolve_into(path, &mut state) {
        Ok(lookup) => lookup,
        Err(err) => {
            crate::debug!(
                "Rejecting set request topic={=str} depth={=usize} err={=?}",
                inbound.topic(),
                err.lookup.depth,
                err.error
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
            return Route::Rejected {
                aftermath: reply.map(|target| Aftermath {
                    phase: AftermathPhase::ErrorReply {
                        target,
                        message: encode_body(&body),
                        op: None,
                    },
                }),
            };
        }
    };

    if inbound.payload().is_empty() {
        crate::debug!("Ignoring empty set payload topic={=str}", inbound.topic());
        return Route::Ignored;
    }

    if !lookup.schema.is_leaf() {
        crate::debug!(
            "Rejecting non-leaf set request topic={=str}",
            inbound.topic()
        );
        let body = ResponseBody::LeafRequired {
            depth: lookup.depth,
        };
        return Route::Rejected {
            aftermath: reply.map(|target| Aftermath {
                phase: AftermathPhase::ErrorReply {
                    target,
                    message: encode_body(&body),
                    op: None,
                },
            }),
        };
    }

    let full = &state[..lookup.depth];
    match Miniconf::<Settings>::with_leaf(full, |keys| {
        json_core::set_by_keys(settings, keys, inbound.payload())
    }) {
        Ok(_) => {
            crate::debug!(
                "Accepted set request topic={=str} depth={=usize} payload_len={=usize} reply={=bool}",
                inbound.topic(),
                lookup.depth,
                inbound.payload().len(),
                reply.is_some()
            );
            let changed = Indices::new(state, lookup.depth);
            Route::Accepted {
                changed,
                aftermath: Aftermath {
                    phase: AftermathPhase::Publish {
                        state: changed,
                        reply,
                        op: None,
                    },
                },
            }
        }
        Err(err) => {
            crate::debug!(
                "Rejecting set request topic={=str} depth={=usize} payload_len={=usize} class={=str}",
                inbound.topic(),
                err.depth,
                inbound.payload().len(),
                match &err.inner {
                    miniconf::SerdeError::Value(_) => "Value",
                    miniconf::SerdeError::Inner(_) => "Deserialize",
                    miniconf::SerdeError::Finalization(_) => "Finalization",
                }
            );
            let body = ResponseBody::Set(err);
            Route::Rejected {
                aftermath: reply.map(|target| Aftermath {
                    phase: AftermathPhase::ErrorReply {
                        target,
                        message: encode_body(&body),
                        op: None,
                    },
                }),
            }
        }
    }
}

impl AftermathPhase {
    pub(crate) async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
        IO: Io,
    {
        loop {
            match self {
                Self::Publish { state, reply, op } => {
                    match super::poll_op(session, op)? {
                        PendingOp::Pending => return Ok(false),
                        PendingOp::Complete => {
                            if let Some(target) = reply.take() {
                                crate::debug!(
                                    "Published authoritative setting; sending MM2 success reply reply_topic={=str}",
                                    target.topic()
                                );
                                *self = Self::ReplyOk { target, op: None };
                                continue;
                            }
                            crate::debug!(
                                "Published authoritative setting without reply topic depth={=usize}",
                                state.as_ref().len()
                            );
                            *self = Self::Done;
                            return Ok(true);
                        }
                        PendingOp::Idle => {}
                    }
                    match mm2.publish_current(session, settings, state.as_ref()).await {
                        Ok(next) => {
                            *op = next;
                            if op.is_none() {
                                continue;
                            }
                            return Ok(false);
                        }
                        Err(Error::Mqtt(MqttError::NotReady))
                        | Err(Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))) =>
                        {
                            return Ok(false);
                        }
                        Err(err) => {
                            if let Some(target) = reply.take() {
                                crate::warn!(
                                    "Authoritative setting publish failed; replying with MM2 error reply_topic={=str}",
                                    target.topic()
                                );
                                let (error, payload) = publish_error_text(&err);
                                *self = Self::ReplyPublishError {
                                    target,
                                    error,
                                    payload,
                                    op: None,
                                };
                                continue;
                            }
                            return Err(err);
                        }
                    }
                }
                Self::ErrorReply {
                    target,
                    message,
                    op,
                } => {
                    match super::poll_op(session, op)? {
                        PendingOp::Pending => return Ok(false),
                        PendingOp::Complete => {
                            crate::debug!(
                                "Completed MM2 error reply reply_topic={=str} kind={=str} depth={=?}",
                                target.topic(),
                                message.kind,
                                message.depth
                            );
                            *self = Self::Done;
                            return Ok(true);
                        }
                        PendingOp::Idle => {}
                    }
                    match reply_message(session, target, message).await {
                        Ok(next) => {
                            *op = next;
                            if op.is_none() {
                                *self = Self::Done;
                                return Ok(true);
                            }
                            return Ok(false);
                        }
                        Err(Error::Mqtt(MqttError::NotReady))
                        | Err(Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))) =>
                        {
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
                Self::ReplyOk { target, op } => {
                    match super::poll_op(session, op)? {
                        PendingOp::Pending => return Ok(false),
                        PendingOp::Complete => {
                            crate::debug!(
                                "Completed MM2 success reply reply_topic={=str}",
                                target.topic()
                            );
                            *self = Self::Done;
                            return Ok(true);
                        }
                        PendingOp::Idle => {}
                    }
                    match reply_text(session, target, ResponseCode::Ok, b"").await {
                        Ok(next) => {
                            *op = next;
                            if op.is_none() {
                                *self = Self::Done;
                                return Ok(true);
                            }
                            return Ok(false);
                        }
                        Err(Error::Mqtt(MqttError::NotReady))
                        | Err(Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))) =>
                        {
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
                Self::ReplyPublishError {
                    target,
                    error,
                    payload,
                    op,
                } => {
                    match super::poll_op(session, op)? {
                        PendingOp::Pending => return Ok(false),
                        PendingOp::Complete => {
                            crate::debug!(
                                "Completed MM2 publish-error reply reply_topic={=str}",
                                target.topic()
                            );
                            *self = Self::Done;
                            return Ok(true);
                        }
                        PendingOp::Idle => {}
                    }
                    match reply_publish_error(session, target, error, payload.as_bytes()).await {
                        Ok(next) => {
                            *op = next;
                            if op.is_none() {
                                *self = Self::Done;
                                return Ok(true);
                            }
                            return Ok(false);
                        }
                        Err(Error::Mqtt(MqttError::NotReady))
                        | Err(Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))) =>
                        {
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
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
        .push(Property::UserProperty(Utf8String(key), Utf8String(value)))
        .ok();
}

async fn reply_message<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    message: &ReplyMessage,
) -> Result<Option<Op>, Error<IO::Error>>
where
    IO: Io,
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
) -> Result<Option<Op>, Error<IO::Error>>
where
    IO: Io,
{
    let props = error_props(ResponseCode::Error, "publish", "Error", error, None);
    reply_bytes(session, target, &props, payload).await
}

async fn reply_text<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    code: ResponseCode,
    text: &[u8],
) -> Result<Option<Op>, Error<IO::Error>>
where
    IO: Io,
{
    let props = [code.into()];
    reply_bytes(session, target, &props, text).await
}

async fn reply_bytes<IO>(
    session: &mut Session<'_, IO>,
    target: &ReplyTarget,
    props: &[Property<'_>],
    payload: &[u8],
) -> Result<Option<Op>, Error<IO::Error>>
where
    IO: Io,
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
