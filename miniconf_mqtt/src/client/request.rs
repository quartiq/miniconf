use core::convert::Infallible;
use core::fmt::{Debug, Write as _};

use heapless::{String, Vec};
use itoa::Buffer;
use log::{debug, warn};
use miniconf::{ConstPath, DescendError, SerdeError, ValueError, json_core};
use minimq::{InboundPublish, Property, ProtocolError, PubError, Publication, QoS};

#[cfg(feature = "compat-settings-ingress")]
use crate::client::SettingsIngressPhase;
use crate::{
    EncodeError, Error, MAX_TOPIC_LENGTH, MqttClient,
    client::Change,
    message::{
        Action, DepthError, ReplyBody, ReplyTarget, Resource, ResponseCode, format_slice,
        simple_pub_error,
    },
};

fn error_props<'a, E: Debug>(
    code: ResponseCode,
    kind: &'static str,
    class: &'static str,
    err: &E,
    depth: Option<usize>,
    error: &'a mut String<96>,
    depth_buf: &'a mut Buffer,
) -> Vec<Property<'a>, 5> {
    let mut props = Vec::new();
    push_prop(&mut props, "code", code.into());
    push_prop(&mut props, "kind", kind);
    push_prop(&mut props, "class", class);
    error.clear();
    write!(error, "{err:?}").ok();
    push_prop(&mut props, "error", error.as_str());
    if let Some(depth) = depth {
        push_prop(&mut props, "depth", depth_buf.format(depth));
    }
    props
}

fn push_prop<'a, const N: usize>(
    props: &mut Vec<Property<'a>, N>,
    key: &'static str,
    value: &'a str,
) {
    props
        .push(Property::UserProperty(
            minimq::types::Utf8String(key),
            minimq::types::Utf8String(value),
        ))
        .ok();
}

impl<'a, Settings, C> MqttClient<'a, Settings, C>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
    C: minimq::transport::Connector,
{
    pub(crate) fn plan_request(
        prefix: &str,
        settings: &mut Settings,
        message: &InboundPublish<'_>,
    ) -> Action {
        let Some((resource, _)) = Resource::parse(message.topic(), prefix) else {
            return Action::Unhandled;
        };

        let reply = match resource {
            Resource::Set => match message
                .response_topic()
                .map(|topic| ReplyTarget::new(topic, message.correlation_data()))
                .transpose()
            {
                Ok(reply) => reply,
                Err(err) => {
                    warn!(
                        "Rejecting request with oversized reply target on {}: {err:?}",
                        message.topic()
                    );
                    return Action::None(Change::Unchanged);
                }
            },
            #[cfg(feature = "compat-settings-ingress")]
            Resource::Settings => None,
        };

        Self::plan_publish(prefix, settings, message.topic(), message.payload(), reply)
    }

    pub(crate) fn plan_publish(
        prefix: &str,
        settings: &mut Settings,
        topic: &str,
        payload: &[u8],
        reply: Option<ReplyTarget>,
    ) -> Action {
        let Some((resource, path)) = Resource::parse(topic, prefix) else {
            return Action::Unhandled;
        };

        let mut state = [0; crate::MAX_DEPTH];
        let lookup = match Settings::SCHEMA.resolve_into(path, &mut state) {
            Ok(lookup) => lookup,
            Err(err) => {
                if matches!(resource, Resource::Set) {
                    debug!("Rejecting set request topic={} err={err:?}", topic);
                    let err = DepthError::<Infallible> {
                        inner: match err.error {
                            DescendError::Key(err) => SerdeError::Value(ValueError::Key(err)),
                            DescendError::Inner(()) => {
                                SerdeError::Value(ValueError::Access("Insufficient state"))
                            }
                        },
                        depth: err.lookup.depth,
                    };
                    return Action::Reply {
                        state: Change::Unchanged,
                        reply,
                        code: ResponseCode::Error,
                        body: ReplyBody::Lookup(err),
                    };
                }
                return Action::None(Change::Unchanged);
            }
        };

        if payload.is_empty() {
            if matches!(resource, Resource::Set) {
                debug!("Ignoring empty set payload topic={topic}");
            }
            return match resource {
                Resource::Set => Action::None(Change::Unchanged),
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings if lookup.schema.is_leaf() => Action::OverrideSet {
                    state,
                    depth: lookup.depth,
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::None(Change::Unchanged),
            };
        }

        if !lookup.schema.is_leaf() {
            if matches!(resource, Resource::Set) {
                debug!("Rejecting non-leaf set request topic={topic}");
            }
            return match resource {
                Resource::Set => Action::Reply {
                    state: Change::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    body: ReplyBody::LeafRequired {
                        depth: lookup.depth,
                    },
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::None(Change::Unchanged),
            };
        }

        let full = &state[..lookup.depth];
        match Self::with_leaf(full, |keys| json_core::set_by_keys(settings, keys, payload)) {
            Ok(_) => Action::PublishSet {
                resource,
                reply,
                state,
                depth: lookup.depth,
            },
            Err(inner) => match resource {
                Resource::Set => Action::Reply {
                    state: Change::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    body: ReplyBody::Set(inner),
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::OverrideSet {
                    state,
                    depth: lookup.depth,
                },
            },
        }
    }

    pub(super) async fn execute(&mut self, settings: &Settings, action: Action) -> Change {
        match action {
            Action::Unhandled => Change::Unchanged,
            Action::None(state) => state,
            Action::Reply {
                state,
                reply,
                code,
                body,
            } => {
                if let Some(reply) = &reply {
                    self.reply_body(reply, code, &body).await;
                }
                state
            }
            Action::PublishSet {
                resource,
                reply,
                state,
                depth,
            } => {
                if matches!(resource, Resource::Set) {
                    if let Err(err) = self.try_publish_leaf(settings, state, depth).await {
                        if let Some(reply) = &reply {
                            let err = simple_pub_error(err);
                            self.reply_publish_error(reply, &err).await;
                        }
                        return Change::Unchanged;
                    }
                    if let Some(reply) = &reply {
                        self.reply_text(reply, ResponseCode::Ok, "").await;
                    }
                    return Change::Changed;
                }

                #[cfg(feature = "compat-settings-ingress")]
                match self.protocol.settings_ingress {
                    SettingsIngressPhase::Recovering(_) => Change::Changed,
                    SettingsIngressPhase::Runtime => {
                        if self.publish_current(settings, state, depth).await.is_err() {
                            return Change::Unchanged;
                        }
                        Change::Changed
                    }
                }
                #[cfg(not(feature = "compat-settings-ingress"))]
                unreachable!()
            }
            #[cfg(feature = "compat-settings-ingress")]
            Action::OverrideSet { state, depth } => match self.protocol.settings_ingress {
                SettingsIngressPhase::Recovering(_) => Change::Unchanged,
                SettingsIngressPhase::Runtime => {
                    let _ = self.publish_current(settings, state, depth).await;
                    Change::Unchanged
                }
            },
        }
    }

    async fn reply_body(&mut self, reply: &ReplyTarget, code: ResponseCode, body: &ReplyBody) {
        let mut error = String::new();
        let mut depth = Buffer::new();
        let props = match body {
            ReplyBody::Lookup(err) => error_props(
                code,
                "lookup",
                "SerdeError",
                &err.inner,
                Some(err.depth),
                &mut error,
                &mut depth,
            ),
            ReplyBody::LeafRequired { depth: value } => error_props(
                code,
                "set",
                "KeyError",
                &miniconf::KeyError::TooShort,
                Some(*value),
                &mut error,
                &mut depth,
            ),
            ReplyBody::Set(err) => error_props(
                code,
                "set",
                "SerdeError",
                &err.inner,
                Some(err.depth),
                &mut error,
                &mut depth,
            ),
        };
        self.reply_with(reply, &props, body).await;
    }

    async fn reply_publish_error(&mut self, reply: &ReplyTarget, err: &Error<C::Error>) {
        let mut error = String::new();
        let mut depth = Buffer::new();
        let props = error_props(
            ResponseCode::Error,
            "publish",
            "Error",
            err,
            None,
            &mut error,
            &mut depth,
        );
        self.reply_with(reply, &props, err).await;
    }

    async fn reply_text(
        &mut self,
        reply: &ReplyTarget,
        code: ResponseCode,
        text: impl core::fmt::Display,
    ) {
        let props = [code.into()];
        self.reply_with(reply, &props, text).await;
    }

    async fn reply_with(
        &mut self,
        reply: &ReplyTarget,
        props: &[minimq::Property<'_>],
        text: impl core::fmt::Display,
    ) {
        if let Err(err) = self
            .session
            .publish(
                reply
                    .publication(|buf: &mut [u8]| {
                        format_slice(text, buf).map_err(|err| (true, err))
                    })
                    .properties(props)
                    .qos(QoS::AtLeastOnce),
            )
            .await
        {
            warn!("Failed to publish reply: {:?}", simple_pub_error(err));
        }
    }

    pub(super) async fn try_publish_leaf(
        &mut self,
        settings: &Settings,
        state: [usize; crate::MAX_DEPTH],
        depth: usize,
    ) -> Result<(), PubError<EncodeError<DepthError<serde_json_core::ser::Error>>, C::Error>> {
        let topic = self
            .settings_topic(&state[..depth])
            .map_err(|err| match err {
                Error::Mqtt(err) => PubError::Session(err),
                Error::Miniconf(_) => unreachable!(),
            })?;
        self.protocol.manifest.settings_rev = self.protocol.manifest.settings_rev.wrapping_add(1);
        let mut rev = Buffer::new();
        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("rev"),
            minimq::types::Utf8String(rev.format(self.protocol.manifest.settings_rev)),
        )];
        let publication = Publication::new(&topic, |buf: &mut [u8]| {
            let full = &state[..depth];
            Self::with_leaf(full, |keys| json_core::get_by_keys(settings, keys, buf)).map_err(
                |err| {
                    let no_space = matches!(
                        err.inner,
                        miniconf::SerdeError::Inner(serde_json_core::ser::Error::BufferFull)
                    );
                    (no_space, err)
                },
            )
        })
        .properties(&props)
        .qos(QoS::AtLeastOnce)
        .retain();
        self.session.publish(publication).await
    }

    pub(super) async fn clear_leaf(&mut self, topic: &str) -> Result<(), Error<C::Error>> {
        self.protocol.manifest.settings_rev = self.protocol.manifest.settings_rev.wrapping_add(1);
        let mut rev = Buffer::new();
        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("rev"),
            minimq::types::Utf8String(rev.format(self.protocol.manifest.settings_rev)),
        )];
        let publication = Publication::bytes(topic, b"")
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        self.session
            .publish(publication)
            .await
            .map_err(simple_pub_error)
    }

    fn settings_topic(&self, state: &[usize]) -> Result<String<MAX_TOPIC_LENGTH>, Error<C::Error>> {
        let path: ConstPath<String<MAX_TOPIC_LENGTH>, '/'> = Settings::SCHEMA
            .transcode(state)
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/settings")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str(path.as_ref())
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        Ok(topic)
    }

    pub(super) async fn publish_current(
        &mut self,
        settings: &Settings,
        state: [usize; crate::MAX_DEPTH],
        depth: usize,
    ) -> Result<(), Error<C::Error>> {
        let topic = self.settings_topic(&state[..depth])?;
        match self.try_publish_leaf(settings, state, depth).await {
            Ok(()) => Ok(()),
            Err(PubError::Payload((
                _no_space,
                DepthError {
                    inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                    ..
                },
            ))) => self.clear_leaf(&topic).await,
            Err(err) => Err(simple_pub_error(err)),
        }
    }
}
