use minimq::{
    Error as MqttError, Io, Op, PubError, Publication, QoS, ResourceError, Session, TopicString,
    types::{SubscriptionOptions, TopicFilter},
};

use crate::{
    Error,
    client::{
        ChangedKey, Miniconf, PayloadError, PendingOp, PublishPayload, Publisher,
        publish_alive_once, schema_page_topic,
    },
    schema::SchemaSync,
};

#[allow(clippy::large_enum_variant)]
pub(crate) enum StartupPhase {
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
    pub(crate) fn new(schema: &'static miniconf::Schema) -> Self {
        Self {
            sync: SchemaSync::new(schema),
            op: None,
        }
    }
}

fn is_retryable_startup_error<E>(err: &Error<E>) -> bool {
    matches!(
        err,
        Error::Mqtt(MqttError::NotReady)
            | Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))
    )
}

impl StartupPhase {
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
                    if schema.step::<Settings, _>(&mm2.prefix, session).await? {
                        mm2.manifest.schema_pages = schema.sync.page;
                        mm2.manifest.schema_rev = schema.sync.hash;
                        crate::debug!(
                            "Schema startup phase complete pages={=usize} rev={=u32}",
                            mm2.manifest.schema_pages,
                            mm2.manifest.schema_rev
                        );
                        *self = Self::Settings(Publisher::root(Settings::SCHEMA));
                        continue;
                    }
                    return Ok(false);
                }
                Self::Settings(publisher) => {
                    if publisher.step(mm2, session, settings).await? {
                        crate::debug!(
                            "Settings startup phase complete rev={=u32}",
                            mm2.manifest.settings_rev
                        );
                        *self = Self::SubscribeSet(None);
                        continue;
                    }
                    return Ok(false);
                }
                Self::SubscribeSet(op) => match super::poll_op(session, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        crate::debug!("Subscribed MM2 request ingress");
                        *self = Self::Alive(None);
                        continue;
                    }
                    PendingOp::Idle => match subscribe_set(&mm2.prefix, session).await {
                        Ok(next) => {
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_retryable_startup_error(&err) => return Ok(false),
                        Err(err) => return Err(err),
                    },
                },
                Self::Alive(op) => match super::poll_op(session, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        crate::info!(
                            "Completed MM2 startup epoch={=u32} schema_rev={=u32} settings_rev={=u32}",
                            mm2.manifest.epoch,
                            mm2.manifest.schema_rev,
                            mm2.manifest.settings_rev
                        );
                        *self = Self::Done;
                        return Ok(true);
                    }
                    PendingOp::Idle => {
                        match publish_alive_once::<Settings, _>(&mm2.prefix, &mm2.manifest, session)
                            .await
                        {
                            Ok(next) => {
                                *op = next;
                                return Ok(false);
                            }
                            Err(err) if is_retryable_startup_error(&err) => return Ok(false),
                            Err(err) => return Err(err),
                        }
                    }
                },
                Self::Done => return Ok(true),
            }
        }
    }
}

impl SchemaPublisher {
    async fn step<Settings, IO>(
        &mut self,
        prefix: &TopicString,
        session: &mut Session<'_, IO>,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: miniconf::TreeSerialize,
        IO: Io,
    {
        match super::poll_op(session, &mut self.op)? {
            PendingOp::Pending => return Ok(false),
            PendingOp::Complete | PendingOp::Idle => {}
        }

        if self.sync.next == self.sync.defs.len() {
            crate::info!(
                "Completed schema sync pages={=usize} rev={=u32}",
                self.sync.page,
                self.sync.hash
            );
            return Ok(true);
        }

        crate::debug!(
            "Publishing schema page={=usize} next_def={=usize} defs_total={=usize}",
            self.sync.page,
            self.sync.next,
            self.sync.defs.len()
        );
        let topic = schema_page_topic(prefix, self.sync.page);
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
        .properties(crate::UTF8_PAYLOAD_PROPERTIES)
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
                crate::info!(
                    "Aborting schema sync after oversized schema entry definition={=usize}",
                    id
                );
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
    loop {
        if publisher.iter.is_none() {
            publisher.iter = Some(crate::schema::SettingsSync::with_root(
                publisher.schema,
                publisher.root.as_ref(),
            )?);
            crate::info!(
                "Starting retained settings sync root_depth={=usize}",
                publisher.root.as_ref().len()
            );
        }

        let state = match publisher.pending {
            Some(state) => state,
            None => {
                let iter = publisher.iter.as_mut().unwrap();
                let Some(path) = iter.next() else {
                    crate::info!(
                        "Completed retained settings sync rev={=u32}",
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
                crate::debug!(
                    "Preparing retained setting publication depth={=usize} rev_next={=u32}",
                    state.as_ref().len(),
                    mm2.manifest.settings_rev.wrapping_add(1)
                );
                state
            }
        };

        match super::poll_op(session, &mut publisher.op)? {
            PendingOp::Pending => return Ok(false),
            PendingOp::Complete => {
                crate::debug!(
                    "Published retained setting depth={=usize} rev={=u32}",
                    state.as_ref().len(),
                    mm2.manifest.settings_rev
                );
                publisher.pending = None;
                continue;
            }
            PendingOp::Idle => {}
        }

        if !session.can_publish(QoS::AtLeastOnce) {
            return Ok(false);
        }

        match mm2.publish_current(session, settings, state.as_ref()).await {
            Ok(op) => {
                publisher.op = op;
                return Ok(false);
            }
            Err(Error::Mqtt(MqttError::NotReady))
            | Err(Error::Mqtt(MqttError::Resource(ResourceError::InflightExhausted))) => {
                return Ok(false);
            }
            Err(err) => return Err(err),
        }
    }
}

async fn subscribe_set<IO>(
    prefix: &TopicString,
    session: &mut Session<'_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/set/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    crate::debug!(
        "Subscribing MM2 request ingress topic={=str}",
        topic.as_str()
    );
    let topics = [
        TopicFilter::new(&topic).options(SubscriptionOptions::default().ignore_local_messages())
    ];
    session.subscribe(&topics, &[]).await.map_err(Into::into)
}
