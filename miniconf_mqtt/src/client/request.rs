use core::convert::Infallible;

use heapless::String;
use itoa::Buffer;
use log::{debug, warn};
use miniconf::{DescendError, FromConfig, Path, SerdeError, Transcode, ValueError, json_core};
use minimq::{InboundPublish, ProtocolError, PubError, Publication, QoS};

#[cfg(feature = "compat-settings-ingress")]
use crate::{
    Error, MAX_TOPIC_LENGTH, MqttClient, SEPARATOR, State,
    client::SettingsIngressPhase,
    message::{
        Action, DepthError, ReplyTarget, Resource, ResponseCode, format_message, simple_pub_error,
    },
    schema::Pending,
};

impl<'a, Settings, C, const Y: usize> MqttClient<'a, Settings, C, Y>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
    C: minimq::transport::Connector,
{
    pub(crate) fn plan_request(
        prefix: &str,
        settings: &mut Settings,
        message: &InboundPublish<'_>,
    ) -> Action<Y> {
        let Some((resource, _)) = Resource::parse(message.topic(), prefix) else {
            return Action::None(State::Unchanged);
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
                    return Action::None(State::Unchanged);
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
    ) -> Action<Y> {
        let Some((resource, path)) = Resource::parse(topic, prefix)
            .map(|(parsed, path)| (parsed, Path::new(path, SEPARATOR)))
        else {
            return Action::None(State::Unchanged);
        };

        let mut state = [0; Y];
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
                        depth: err.depth,
                    };
                    return Action::Reply {
                        state: State::Unchanged,
                        reply,
                        code: ResponseCode::Error,
                        text: format_message(err),
                    };
                }
                return Action::None(State::Unchanged);
            }
        };

        if payload.is_empty() {
            if matches!(resource, Resource::Set) {
                debug!("Ignoring empty set payload topic={topic}");
            }
            return match resource {
                Resource::Set => Action::None(State::Unchanged),
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings if lookup.schema.is_leaf() => Action::OverrideSet {
                    state,
                    depth: lookup.depth,
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::None(State::Unchanged),
            };
        }

        if !lookup.schema.is_leaf() {
            if matches!(resource, Resource::Set) {
                debug!("Rejecting non-leaf set request topic={topic}");
            }
            return match resource {
                Resource::Set => Action::Reply {
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message("Path does not resolve to a leaf"),
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::None(State::Unchanged),
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
                    state: State::Unchanged,
                    reply,
                    code: ResponseCode::Error,
                    text: format_message(inner),
                },
                #[cfg(feature = "compat-settings-ingress")]
                Resource::Settings => Action::OverrideSet {
                    state,
                    depth: lookup.depth,
                },
            },
        }
    }

    pub(super) async fn execute(&mut self, settings: &Settings, action: Action<Y>) -> State {
        match action {
            Action::None(state) => state,
            Action::Reply {
                state,
                reply,
                code,
                text,
            } => {
                if let Some(reply) = &reply {
                    self.reply_text(reply, code, text.as_str()).await;
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
                            self.reply_text(
                                reply,
                                ResponseCode::Error,
                                format_message(simple_pub_error(err)).as_str(),
                            )
                            .await;
                        }
                        return State::Unchanged;
                    }
                    self.queue_settings_sync();
                    if let Some(reply) = &reply {
                        self.reply_text(reply, ResponseCode::Ok, "").await;
                    }
                    return State::Changed;
                }

                #[cfg(feature = "compat-settings-ingress")]
                match self.settings_ingress {
                    SettingsIngressPhase::Recovering { .. } => State::Changed,
                    SettingsIngressPhase::Runtime => {
                        if self.publish_current(settings, state, depth).await.is_err() {
                            return State::Unchanged;
                        }
                        self.queue_settings_sync();
                        State::Changed
                    }
                }
                #[cfg(not(feature = "compat-settings-ingress"))]
                unreachable!()
            }
            #[cfg(feature = "compat-settings-ingress")]
            Action::OverrideSet { state, depth } => match self.settings_ingress {
                SettingsIngressPhase::Recovering { .. } => State::Unchanged,
                SettingsIngressPhase::Runtime => {
                    let _ = self.publish_current(settings, state, depth).await;
                    State::Unchanged
                }
            },
        }
    }

    fn queue_settings_sync(&mut self) {
        if matches!(self.pending, Pending::Idle) {
            debug!("Queued retained settings sync");
            self.pending = Pending::settings(Settings::SCHEMA);
        }
    }

    async fn reply_text(&mut self, reply: &ReplyTarget, code: ResponseCode, text: &str) {
        let props = [code.into()];
        if let Err(err) = self
            .session
            .publish(
                reply
                    .publication(text.as_bytes())
                    .properties(&props)
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
        state: [usize; Y],
        depth: usize,
    ) -> Result<(), PubError<DepthError<serde_json_core::ser::Error>, C::Error>> {
        let topic = self
            .settings_topic(&state[..depth])
            .map_err(|err| match err {
                Error::Mqtt(err) => PubError::Session(err),
                Error::Miniconf(_) => unreachable!(),
            })?;
        self.rev = self.rev.wrapping_add(1);
        let mut rev = Buffer::new();
        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("rev"),
            minimq::types::Utf8String(rev.format(self.rev)),
        )];
        let publication = Publication::new(&topic, |buf: &mut [u8]| {
            let full = &state[..depth];
            Self::with_leaf(full, |keys| json_core::get_by_keys(settings, keys, buf))
        })
        .properties(&props)
        .qos(QoS::AtLeastOnce)
        .retain();
        self.session.publish(publication).await
    }

    pub(super) async fn clear_leaf(&mut self, topic: &str) -> Result<(), Error<C::Error>> {
        self.rev = self.rev.wrapping_add(1);
        let mut rev = Buffer::new();
        let props = [minimq::Property::UserProperty(
            minimq::types::Utf8String("rev"),
            minimq::types::Utf8String(rev.format(self.rev)),
        )];
        let publication = Publication::new(topic, b"")
            .properties(&props)
            .qos(QoS::AtLeastOnce)
            .retain();
        self.session
            .publish(publication)
            .await
            .map_err(simple_pub_error)
    }

    fn settings_topic(&self, state: &[usize]) -> Result<String<MAX_TOPIC_LENGTH>, Error<C::Error>> {
        let mut path = Path::<String<MAX_TOPIC_LENGTH>>::from_config(&SEPARATOR);
        path.transcode_from(Settings::SCHEMA, state)
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

    #[cfg(feature = "compat-settings-ingress")]
    async fn publish_current(
        &mut self,
        settings: &Settings,
        state: [usize; Y],
        depth: usize,
    ) -> Result<(), Error<C::Error>> {
        let topic = self.settings_topic(&state[..depth])?;
        match self.try_publish_leaf(settings, state, depth).await {
            Ok(()) => Ok(()),
            Err(PubError::Payload(DepthError {
                inner: SerdeError::Value(ValueError::Absent | ValueError::Access(_)),
                ..
            })) => self.clear_leaf(&topic).await,
            Err(err) => Err(simple_pub_error(err)),
        }
    }
}
