use core::convert::Infallible;
use core::fmt::Write as _;

use heapless::{String, Vec, VecView};
use miniconf::{
    DescendError, Indices, KeyError, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    ValueError, json_core,
};
use minimq::{
    Error as MqttError, InboundPublish, Io, Op, Property, QoS, ResourceError, Session,
    types::Utf8String,
};
use serde_json_core::de::Error as JsonDeError;

use crate::{
    Error,
    client::{ChangedKey, Miniconf, PendingOp, ReplyTarget, Route},
    message::{DepthError, ResponseBody, ResponseCode, set_path, settings_path, simple_pub_error},
};

type ResponseText = String<{ crate::RESPONSE_TEXT_LENGTH }>;

pub(crate) enum FollowUp {
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
        error: ResponseText,
        payload: ResponseText,
        op: Option<Op>,
    },
    Done,
}

impl FollowUp {
    fn publish(state: ChangedKey, reply: Option<ReplyTarget>) -> Self {
        Self::Publish {
            state,
            reply,
            op: None,
        }
    }
}

pub(crate) struct ReplyMessage {
    code: ResponseCode,
    kind: &'static str,
    class: &'static str,
    error: ResponseText,
    depth: Option<usize>,
    payload: ResponseText,
}

pub(crate) enum Rev {
    Absent,
    Valid,
    Invalid,
}

pub(crate) fn needs_capacity<Settings>(prefix: &str, inbound: &InboundPublish<'_>) -> bool
where
    Settings: TreeSchema,
{
    if set_path(inbound.topic(), prefix).is_some() {
        return true;
    }
    let Some(path) = settings_path(inbound.topic(), prefix) else {
        return false;
    };
    if !matches!(rev(inbound), Rev::Absent) {
        return false;
    }
    let mut state = [0; crate::MAX_DEPTH];
    resolve_leaf::<Settings>(path, &mut state).is_some()
}

fn with_leaf<T, E>(
    full: &[usize],
    func: impl FnOnce(&mut &[usize]) -> Result<T, SerdeError<E>>,
) -> Result<T, DepthError<E>> {
    let mut keys = full;
    func(&mut keys).map_err(|inner| DepthError {
        inner,
        depth: full.len() - keys.len(),
    })
}

pub(crate) fn route<Settings>(
    prefix: &str,
    settings: &mut Settings,
    inbound: &InboundPublish<'_>,
) -> Route
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
{
    if let Some(path) = settings_path(inbound.topic(), prefix) {
        return route_settings(settings, inbound, path);
    }

    let Some(path) = set_path(inbound.topic(), prefix) else {
        return Route::Unhandled;
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
                follow_up: reply.map(|target| FollowUp::ErrorReply {
                    target,
                    message: encode_body(&body),
                    op: None,
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
            follow_up: reply.map(|target| FollowUp::ErrorReply {
                target,
                message: encode_body(&body),
                op: None,
            }),
        };
    }

    let full = &state[..lookup.depth];
    match with_leaf(full, |keys| {
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
                follow_up: FollowUp::publish(changed, reply),
            }
        }
        Err(err) => {
            crate::debug!(
                "Rejecting set request topic={=str} depth={=usize} payload_len={=usize} class={=str}",
                inbound.topic(),
                err.depth,
                inbound.payload().len(),
                match &err.inner {
                    SerdeError::Value(_) => "Value",
                    SerdeError::Inner(_) => "Deserialize",
                    SerdeError::Finalization(_) => "Finalization",
                }
            );
            let body = ResponseBody::Set(err);
            Route::Rejected {
                follow_up: reply.map(|target| FollowUp::ErrorReply {
                    target,
                    message: encode_body(&body),
                    op: None,
                }),
            }
        }
    }
}

pub(crate) fn rev(inbound: &InboundPublish<'_>) -> Rev {
    let mut seen = false;
    for property in inbound.properties().iter() {
        let Ok(Property::UserProperty(key, value)) = property else {
            continue;
        };
        if key.0 != "rev" {
            continue;
        }
        if seen || value.0.parse::<u32>().is_err() {
            return Rev::Invalid;
        }
        seen = true;
    }
    if seen { Rev::Valid } else { Rev::Absent }
}

pub(crate) fn resolve_leaf<Settings>(
    path: &str,
    state: &mut [usize; crate::MAX_DEPTH],
) -> Option<usize>
where
    Settings: TreeSchema,
{
    let lookup = Settings::SCHEMA.resolve_into(path, state).ok()?;
    lookup.schema.is_leaf().then_some(lookup.depth)
}

pub(crate) fn set_leaf<Settings>(
    settings: &mut Settings,
    full: &[usize],
    payload: &[u8],
) -> Result<(), DepthError<JsonDeError>>
where
    Settings: TreeDeserializeOwned,
{
    with_leaf(full, |keys| json_core::set_by_keys(settings, keys, payload)).map(|_| ())
}

fn route_settings<Settings>(
    settings: &mut Settings,
    inbound: &InboundPublish<'_>,
    path: &str,
) -> Route
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
{
    // No-rev settings publications are a narrow compatibility ingress for tools that edit the
    // retained mirror by hand. Rev-bearing publications are the authoritative mirror itself.
    if !matches!(rev(inbound), Rev::Absent) {
        crate::debug!(
            "Ignoring authoritative settings mirror publication topic={=str}",
            inbound.topic()
        );
        return Route::Ignored;
    }

    let mut state = [0; crate::MAX_DEPTH];
    let Some(depth) = resolve_leaf::<Settings>(path, &mut state) else {
        crate::debug!(
            "Ignoring compatibility settings ingress with invalid path topic={=str}",
            inbound.topic()
        );
        return Route::Ignored;
    };

    let changed = Indices::new(state, depth);
    if inbound.payload().is_empty() {
        crate::debug!(
            "Overwriting empty compatibility settings ingress topic={=str}",
            inbound.topic()
        );
        return Route::Rejected {
            follow_up: Some(FollowUp::publish(changed, None)),
        };
    }

    match set_leaf(settings, changed.as_ref(), inbound.payload()) {
        Ok(()) => {
            crate::debug!(
                "Accepted compatibility settings ingress topic={=str} depth={=usize} payload_len={=usize}",
                inbound.topic(),
                depth,
                inbound.payload().len()
            );
            Route::Accepted {
                changed,
                follow_up: FollowUp::publish(changed, None),
            }
        }
        Err(err) => {
            crate::debug!(
                "Overwriting failed compatibility settings ingress topic={=str} depth={=usize} payload_len={=usize} class={=str}",
                inbound.topic(),
                err.depth,
                inbound.payload().len(),
                match &err.inner {
                    SerdeError::Value(_) => "Value",
                    SerdeError::Inner(_) => "Deserialize",
                    SerdeError::Finalization(_) => "Finalization",
                }
            );
            Route::Rejected {
                follow_up: Some(FollowUp::publish(changed, None)),
            }
        }
    }
}

impl FollowUp {
    pub(crate) async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
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
            write!(error.as_mut_view(), "{:?}", err.inner).ok();
            write!(payload.as_mut_view(), "{err}").ok();
            ("lookup", "SerdeError", Some(err.depth))
        }
        ResponseBody::LeafRequired { depth } => {
            write!(error.as_mut_view(), "{:?}", KeyError::TooShort).ok();
            payload.push_str("Path does not resolve to a leaf").ok();
            ("set", "KeyError", Some(*depth))
        }
        ResponseBody::Set(err) => {
            write!(error.as_mut_view(), "{:?}", err.inner).ok();
            write!(payload.as_mut_view(), "{err}").ok();
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

fn publish_error_text<E: core::fmt::Debug>(err: &Error<E>) -> (ResponseText, ResponseText) {
    let mut error = String::new();
    write!(error.as_mut_view(), "{err:?}").ok();
    let mut payload = String::new();
    write!(payload.as_mut_view(), "{err}").ok();
    (error, payload)
}

fn error_props<'a>(
    code: ResponseCode,
    kind: &'static str,
    class: &'static str,
    error: &'a str,
    depth: Option<&'a str>,
) -> Vec<Property<'a>, 7> {
    let mut props = Vec::new();
    push_transient_text_props(&mut props);
    push_prop(&mut props, "code", code.as_str());
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

fn push_transient_text_props(props: &mut VecView<Property<'_>>) {
    for prop in crate::TRANSIENT_TEXT_PROPERTIES {
        props.push(prop.clone()).ok();
    }
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
        write!(depth_text.as_mut_view(), "{value}").ok()?;
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
    let mut props = Vec::<_, 3>::new();
    push_transient_text_props(&mut props);
    props.push(code.into()).ok();
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
