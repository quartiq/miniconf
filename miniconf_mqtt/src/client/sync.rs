use log::{debug, info};
use minimq::{
    Error as MqttError, Io, Op, PubError, Publication, QoS, ResourceError, Session,
    types::{SubscriptionOptions, TopicFilter},
};

use crate::{
    Error,
    client::{ChangedKey, Miniconf, PayloadError, PendingOp, PublishPayload, Publisher},
    schema::SchemaSync,
};

#[allow(clippy::large_enum_variant)]
pub(crate) enum ActivationPhase {
    Schema(SchemaPublisher),
    Settings(Publisher),
    SubscribeSet(Option<Op>),
    Alive(Option<Op>),
    Done,
}

pub(crate) struct SchemaPublisher {
    sync: SchemaSync,
    op: Option<Op>,
}

impl SchemaPublisher {
    pub(crate) fn new<Settings: miniconf::TreeSchema>() -> Self {
        Self {
            sync: SchemaSync::new(Settings::SCHEMA),
            op: None,
        }
    }
}

fn is_retryable_activation_error<E>(err: &Error<E>) -> bool {
    matches!(
        err,
        Error::Mqtt(MqttError::NotReady)
            | Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))
    )
}

impl ActivationPhase {
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
                Self::Schema(schema) => {
                    if schema.step(mm2, session).await? {
                        mm2.manifest.schema_pages = schema.sync.page;
                        mm2.manifest.schema_rev = schema.sync.hash;
                        *self = Self::Settings(Publisher {
                            root: ChangedKey::new([0; crate::MAX_DEPTH], 0),
                            iter: None,
                            pending: None,
                            op: None,
                        });
                        continue;
                    }
                    return Ok(false);
                }
                Self::Settings(publisher) => {
                    if publisher.step(mm2, session, settings).await? {
                        *self = Self::SubscribeSet(None);
                        continue;
                    }
                    return Ok(false);
                }
                Self::SubscribeSet(op) => match super::poll_op(session, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        *self = Self::Alive(None);
                        continue;
                    }
                    PendingOp::Idle => match subscribe_set(mm2, session).await {
                        Ok(next) => {
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_retryable_activation_error(&err) => return Ok(false),
                        Err(err) => return Err(err),
                    },
                },
                Self::Alive(op) => match super::poll_op(session, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        *self = Self::Done;
                        return Ok(true);
                    }
                    PendingOp::Idle => match mm2.publish_alive_once(session).await {
                        Ok(next) => {
                            *op = next;
                            return Ok(false);
                        }
                        Err(err) if is_retryable_activation_error(&err) => return Ok(false),
                        Err(err) => return Err(err),
                    },
                },
                Self::Done => return Ok(true),
            }
        }
    }
}

impl SchemaPublisher {
    async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
        IO: Io,
    {
        match super::poll_op(session, &mut self.op)? {
            PendingOp::Pending => return Ok(false),
            PendingOp::Complete | PendingOp::Idle => {}
        }

        if self.sync.next == self.sync.defs.len() {
            info!(
                "Completed schema sync pages={} rev={}",
                self.sync.page, self.sync.hash
            );
            return Ok(true);
        }

        debug!(
            "Publishing schema page={} next_def={}",
            self.sync.page, self.sync.next
        );
        let topic = mm2.schema_page_topic(self.sync.page);
        let mut advanced = None::<(usize, u32)>;
        let publication: Publication<'_, PublishPayload<'_, '_, Settings>> = Publication::new(
            &topic,
            PublishPayload::SchemaPage {
                defs: &self.sync.defs,
                next: self.sync.next,
                hash: self.sync.hash,
                advanced: &mut advanced,
            },
        )
        .qos(QoS::AtLeastOnce)
        .retain();
        match session.publish(publication).await {
            Ok(op) => {
                let Some((count, hash)) = advanced else {
                    return Err(Error::Mqtt(ResourceError::BufferTooSmall.into()));
                };
                self.sync.next += count;
                self.sync.page += 1;
                self.sync.hash = hash;
                self.op = op;
                Ok(false)
            }
            Err(PubError::Session(MqttError::NotReady))
            | Err(PubError::Session(MqttError::Resource(ResourceError::InflightExhausted))) => {
                Ok(false)
            }
            Err(PubError::Payload((true, PayloadError::Schema(id)))) => {
                info!("Aborting schema sync after oversized schema entry for definition {id}");
                Err(Error::Mqtt(ResourceError::PacketTooLarge.into()))
            }
            Err(PubError::Payload(_)) => Err(Error::Mqtt(ResourceError::BufferTooSmall.into())),
            Err(PubError::Session(err)) => Err(Error::Mqtt(err)),
        }
    }
}

pub(crate) async fn step_publisher<Settings, IO>(
    publisher: &mut Publisher,
    mm2: &mut Miniconf<Settings>,
    session: &mut Session<'_, IO>,
    settings: &Settings,
) -> Result<bool, Error<IO::Error>>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
    IO: Io,
{
    if publisher.iter.is_none() {
        publisher.iter = Some(crate::schema::SettingsSync::with_root(
            Settings::SCHEMA,
            publisher.root.as_ref(),
        )?);
        info!("Starting retained settings sync");
    }

    let state = match publisher.pending {
        Some(state) => state,
        None => {
            let iter = publisher.iter.as_mut().unwrap();
            let Some(path) = iter.next() else {
                info!(
                    "Completed retained settings sync rev={}",
                    mm2.manifest.settings_rev
                );
                return Ok(true);
            };
            path.map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
            let full = iter
                .indices()
                .ok_or_else(|| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
            let mut state = [0; crate::MAX_DEPTH];
            state[..full.len()].copy_from_slice(full);
            let state = ChangedKey::new(state, full.len());
            publisher.pending = Some(state);
            state
        }
    };

    match super::poll_op(session, &mut publisher.op)? {
        PendingOp::Pending => return Ok(false),
        PendingOp::Complete => {
            debug!(
                "Published retained setting {}",
                mm2.settings_topic(state.as_ref())
                    .map_err(MqttError::from)
                    .map_err(Error::from)?
            );
            publisher.op = None;
            publisher.pending = None;
            return Ok(false);
        }
        PendingOp::Idle => {}
    }

    if !session.can_publish(QoS::AtLeastOnce) {
        return Ok(false);
    }

    match mm2.publish_current(session, settings, state.as_ref()).await {
        Ok(op) => {
            publisher.op = op;
            Ok(false)
        }
        Err(Error::Mqtt(MqttError::NotReady))
        | Err(Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))) => Ok(false),
        Err(err) => Err(err),
    }
}

async fn subscribe_set<Settings, IO>(
    mm2: &Miniconf<Settings>,
    session: &mut Session<'_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = mm2.prefix.clone();
    topic
        .push_str("/set/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    let topics = [
        TopicFilter::new(&topic).options(SubscriptionOptions::default().ignore_local_messages())
    ];
    session.subscribe(&topics, &[]).await.map_err(Into::into)
}
