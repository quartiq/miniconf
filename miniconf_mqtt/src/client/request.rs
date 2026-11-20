use core::convert::Infallible;
use core::fmt::Write as _;

use heapless::String;
use miniconf::{
    DescendError, Indices, KeyError, SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize,
    ValueError, json_core,
};
use minimq::{Connection, Error as MqttError, InboundPublish, Io, Op, Property, QoS};
use serde_json_core::de::Error as JsonDeError;

use super::{is_backpressure, poll_op};
use crate::{
    Error, MAX_DEPTH, MAX_TOPIC_LENGTH, RESPONSE_CORRELATION_LENGTH, RESPONSE_TEXT_LENGTH,
    TRANSIENT_TEXT_PROPERTIES,
    client::{ChangedKey, Miniconf, PendingOp, ReplyTarget, Route},
    debug,
    message::{DepthError, ResponseBody, set_path, settings_path, simple_pub_error},
    warn,
};

type ResponseText = String<{ RESPONSE_TEXT_LENGTH }>;

pub(crate) enum FollowUp {
    Publish {
        state: ChangedKey,
        reply: Option<ReplyTarget>,
        op: Option<super::PendingOperation>,
    },
    Reply {
        target: ReplyTarget,
        error: Option<ReplyError>,
        op: Option<super::PendingOperation>,
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

pub(crate) struct ReplyError {
    kind: &'static str,
    class: &'static str,
    error: ResponseText,
    depth: Option<usize>,
    payload: ResponseText,
}

pub(crate) enum Auth {
    Absent,
    Valid,
    Invalid,
}

pub(crate) fn route<Settings>(
    prefix: &str,
    settings: &mut Settings,
    inbound: &InboundPublish<'_>,
    full: bool,
) -> Route
where
    Settings: TreeDeserializeOwned,
{
    if let Some(path) = settings_path(inbound.topic(), prefix) {
        return route_settings(settings, inbound, path, full);
    }

    let Some(path) = set_path(inbound.topic(), prefix) else {
        return Route::Unhandled;
    };
    if full {
        return Route::Busy;
    }

    let reply = match inbound.reply_owned::<{ MAX_TOPIC_LENGTH }, { RESPONSE_CORRELATION_LENGTH }>()
    {
        Ok(reply) => reply,
        Err(err) => {
            warn!(
                "Rejecting request with oversized reply target topic={=str} err={}",
                inbound.topic(),
                err
            );
            return Route::Ignored;
        }
    };

    if inbound.retained() {
        return Route::Rejected {
            follow_up: reply.map(|target| FollowUp::Reply {
                target,
                error: Some(encode_body(&ResponseBody::Retained)),
                op: None,
            }),
        };
    }

    let mut state = [0; MAX_DEPTH];
    let lookup = match Settings::SCHEMA.resolve_into(path, &mut state) {
        Ok(lookup) => lookup,
        Err(err) => {
            debug!(
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
                follow_up: reply.map(|target| FollowUp::Reply {
                    target,
                    error: Some(encode_body(&body)),
                    op: None,
                }),
            };
        }
    };

    if inbound.payload().is_empty() {
        debug!("Ignoring empty set payload topic={=str}", inbound.topic());
        return Route::Ignored;
    }

    if !lookup.schema.is_leaf() {
        debug!(
            "Rejecting non-leaf set request topic={=str}",
            inbound.topic()
        );
        let body = ResponseBody::LeafRequired {
            depth: lookup.depth,
        };
        return Route::Rejected {
            follow_up: reply.map(|target| FollowUp::Reply {
                target,
                error: Some(encode_body(&body)),
                op: None,
            }),
        };
    }

    let full = &state[..lookup.depth];
    match set_leaf(settings, full, inbound.payload()) {
        Ok(()) => {
            debug!(
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
            debug!(
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
                follow_up: reply.map(|target| FollowUp::Reply {
                    target,
                    error: Some(encode_body(&body)),
                    op: None,
                }),
            }
        }
    }
}

pub(crate) fn auth(inbound: &InboundPublish<'_>) -> Auth {
    let mut seen = false;
    for property in inbound.properties().iter() {
        let Ok(Property::UserProperty(key, value)) = property else {
            continue;
        };
        if key != "auth" {
            continue;
        }
        if seen || !value.is_empty() {
            return Auth::Invalid;
        }
        seen = true;
    }
    if seen { Auth::Valid } else { Auth::Absent }
}

pub(crate) fn resolve_leaf<Settings>(path: &str, state: &mut [usize; MAX_DEPTH]) -> Option<usize>
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
    let mut keys = full;
    json_core::set_by_keys(settings, &mut keys, payload)
        .map(|_| ())
        .map_err(|inner| DepthError {
            inner,
            depth: full.len() - keys.len(),
        })
}

fn route_settings<Settings>(
    settings: &mut Settings,
    inbound: &InboundPublish<'_>,
    path: &str,
    full: bool,
) -> Route
where
    Settings: TreeDeserializeOwned,
{
    // Compatibility writes are non-retained and carry no auth property.
    if inbound.retained() || !matches!(auth(inbound), Auth::Absent) {
        debug!(
            "Ignoring retained or authoritative settings publication topic={=str}",
            inbound.topic()
        );
        return Route::Ignored;
    }

    let mut state = [0; MAX_DEPTH];
    let Some(depth) = resolve_leaf::<Settings>(path, &mut state) else {
        debug!(
            "Ignoring compatibility settings ingress with invalid path topic={=str}",
            inbound.topic()
        );
        return Route::Ignored;
    };
    if full {
        return Route::Busy;
    }

    let changed = Indices::new(state, depth);
    if inbound.payload().is_empty() {
        debug!(
            "Overwriting empty compatibility settings ingress topic={=str}",
            inbound.topic()
        );
        return Route::Rejected {
            follow_up: Some(FollowUp::publish(changed, None)),
        };
    }

    match set_leaf(settings, changed.as_ref(), inbound.payload()) {
        Ok(()) => {
            debug!(
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
            debug!(
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
        miniconf: &Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        loop {
            match self {
                Self::Publish { state, reply, op } => {
                    match poll_op(connection, op)? {
                        PendingOp::Pending => return Ok(false),
                        PendingOp::Complete => {
                            if let Some(target) = reply.take() {
                                debug!(
                                    "Published authoritative setting; sending Miniconf success reply reply_topic={=str}",
                                    target.topic()
                                );
                                *self = Self::Reply {
                                    target,
                                    error: None,
                                    op: None,
                                };
                                continue;
                            }
                            debug!(
                                "Published authoritative setting without reply topic depth={=usize}",
                                state.as_ref().len()
                            );
                            *self = Self::Done;
                            return Ok(true);
                        }
                        PendingOp::Idle => {}
                    }
                    match miniconf
                        .publish_current(connection, settings, state.as_ref())
                        .await
                    {
                        Ok(next) => {
                            *op = Some(next.into());
                            return Ok(false);
                        }
                        Err(err) if is_backpressure(connection, &err) => return Ok(false),
                        Err(err) => {
                            if let Some(error) = publish_error(&err)
                                && let Some(target) = reply.take()
                            {
                                warn!(
                                    "Authoritative setting publish failed; replying with Miniconf error reply_topic={=str}",
                                    target.topic()
                                );
                                *self = Self::Reply {
                                    target,
                                    error: Some(error),
                                    op: None,
                                };
                                continue;
                            }
                            return Err(err);
                        }
                    }
                }
                Self::Reply { target, error, op } => {
                    match poll_op(connection, op)? {
                        PendingOp::Pending => return Ok(false),
                        PendingOp::Complete => {
                            debug!(
                                "Completed Miniconf reply reply_topic={=str}",
                                target.topic()
                            );
                            *self = Self::Done;
                            return Ok(true);
                        }
                        PendingOp::Idle => {}
                    }
                    match reply(connection, target, error.as_ref()).await {
                        Ok(next) => {
                            *op = Some(next.into());
                            return Ok(false);
                        }
                        Err(err) if is_backpressure(connection, &err) => return Ok(false),
                        Err(err) => return Err(err),
                    }
                }
                Self::Done => return Ok(true),
            }
        }
    }
}

fn encode_body(body: &ResponseBody) -> ReplyError {
    let mut error = String::new();
    let mut payload = String::new();
    let (kind, class, depth) = match body {
        ResponseBody::Retained => {
            payload.push_str("Retained requests are not accepted").ok();
            error.push_str("Retained").ok();
            ("set", "RequestError", None)
        }
        ResponseBody::Lookup(err) => {
            write!(payload.as_mut_view(), "{err}").ok();
            write!(error.as_mut_view(), "{:?}", err.inner).ok();
            ("lookup", "SerdeError", Some(err.depth))
        }
        ResponseBody::LeafRequired { depth } => {
            payload.push_str("Path does not resolve to a leaf").ok();
            write!(error.as_mut_view(), "{:?}", KeyError::TooShort).ok();
            ("set", "KeyError", Some(*depth))
        }
        ResponseBody::Set(err) => {
            write!(payload.as_mut_view(), "{err}").ok();
            write!(error.as_mut_view(), "{:?}", err.inner).ok();
            ("set", "SerdeError", Some(err.depth))
        }
    };
    ReplyError {
        kind,
        class,
        error,
        depth,
        payload,
    }
}

fn publish_error<E>(err: &Error<E>) -> Option<ReplyError> {
    let mut error = String::new();
    let mut payload = String::new();
    let class = match err {
        Error::Tree(err) => {
            write!(error.as_mut_view(), "{err:?}").ok();
            write!(payload.as_mut_view(), "{err}").ok();
            "DescendError"
        }
        Error::Mqtt(MqttError::Resource(err)) => {
            write!(error.as_mut_view(), "{err:?}").ok();
            write!(payload.as_mut_view(), "{err}").ok();
            "ResourceError"
        }
        // Session and transport failures belong to the connection owner. Another
        // publication cannot reliably report them over the same connection.
        _ => return None,
    };
    Some(ReplyError {
        kind: "publish",
        class,
        error,
        depth: None,
        payload,
    })
}

async fn reply<IO>(
    connection: &mut Connection<'_, '_, IO>,
    target: &ReplyTarget,
    error: Option<&ReplyError>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut depth_text = String::<20>::new();
    let [format, expiry] = TRANSIENT_TEXT_PROPERTIES;
    let mut props = [
        format.clone(),
        expiry.clone(),
        Property::UserProperty("code", "Ok"),
        Property::UserProperty("kind", ""),
        Property::UserProperty("class", ""),
        Property::UserProperty("error", ""),
        Property::UserProperty("depth", ""),
    ];
    let (payload, count) = if let Some(error) = error {
        props[2] = Property::UserProperty("code", "Error");
        props[3] = Property::UserProperty("kind", error.kind);
        props[4] = Property::UserProperty("class", error.class);
        props[5] = Property::UserProperty("error", error.error.as_str());
        let count = if let Some(depth) = error.depth {
            // A decimal usize fits in 20 bytes on all supported targets.
            write!(depth_text.as_mut_view(), "{depth}").ok();
            props[6] = Property::UserProperty("depth", &depth_text);
            7
        } else {
            6
        };
        (error.payload.as_bytes(), count)
    } else {
        (&b""[..], 3)
    };
    connection
        .publish(
            target
                .publication(payload)
                .properties(&props[..count])
                .qos(QoS::AtLeastOnce),
        )
        .await
        .map_err(simple_pub_error)?
        .ok_or(Error::Mqtt(MqttError::InvalidRequest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_replies_report_local_failures_only() {
        // Transport errors need no text formatter: they go back to the owner.
        struct Transport;
        let local: Error<Transport> =
            Error::Mqtt(MqttError::Resource(minimq::ResourceError::BufferTooSmall));
        let reply = publish_error(&local).unwrap();
        assert_eq!(reply.kind, "publish");
        assert_eq!(reply.class, "ResourceError");
        assert!(!reply.error.is_empty());
        assert!(!reply.payload.is_empty());

        let tree: Error<Transport> = Error::Tree(miniconf::DescendError::Inner(()));
        assert_eq!(publish_error(&tree).unwrap().class, "DescendError");
        for error in [
            MqttError::Transport(Transport),
            MqttError::Disconnected,
            MqttError::NotReady,
            MqttError::WriteZero,
            MqttError::MissingCompletion,
            MqttError::InvalidRequest,
        ] {
            assert!(publish_error(&Error::Mqtt(error)).is_none());
        }
    }
}
