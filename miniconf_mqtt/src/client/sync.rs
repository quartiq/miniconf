use embassy_time::{Duration, Instant, with_deadline};
use miniconf::{SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize};
use minimq::{
    Error as MqttError, InboundPublish, Io, Op, OpStatus, PubError, Publication, QoS,
    ResourceError, Retain, Session, TopicString,
    types::{RetainHandling, SubscriptionOptions, TopicFilter},
};

use super::request::{Rev, resolve_leaf, rev, set_leaf};
use crate::{
    Error,
    client::{
        ChangedKey, Miniconf, PayloadError, PendingOp, PublishPayload, Publisher,
        publish_alive_once, schema_page_topic,
    },
    message::settings_path,
    schema::{SchemaSync, SettingsSync},
};

#[allow(clippy::large_enum_variant)]
pub(crate) enum StartupPhase {
    Schema { sync: SchemaSync, op: Option<Op> },
    Settings(Publisher),
    SubscribeSet(Option<Op>),
    Alive(Option<Op>),
    Done,
}

pub(crate) enum LoadRetainedPhase {
    Subscribe { start: Instant, op: Option<Op> },
    Drain { deadline: Instant, quiet: Duration },
    Unsubscribe(Option<Op>),
    Done,
}

impl LoadRetainedPhase {
    pub(crate) fn new() -> Self {
        Self::Subscribe {
            start: Instant::now(),
            op: None,
        }
    }

    pub(crate) async fn step<Settings, IO>(
        &mut self,
        mm2: &mut Miniconf<Settings>,
        session: &mut Session<'_, IO>,
        settings: &mut Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        // This state machine is intentionally not feature-gated. It has no extra dependencies, and
        // generic code here is only monomorphized by callers that actually construct LoadRetained.
        // It is a cold-boot recovery step, not reconnect handling: after a running device loses the
        // network, the live settings in RAM remain authoritative and Startup::connected republishes
        // them if the broker session was lost.
        loop {
            match self {
                Self::Subscribe { start, op } => {
                    if let Some(current) = *op {
                        match session.status(&current) {
                            OpStatus::Pending => {
                                if let Some(inbound) = session.poll().await? {
                                    apply_retained(mm2.prefix.as_str(), settings, &inbound);
                                }
                                return Ok(false);
                            }
                            OpStatus::Complete => {
                                let now = Instant::now();
                                let suback_rtt = now.saturating_duration_since(*start);
                                let quiet = retained_quiet_window(suback_rtt);
                                crate::debug!(
                                    "Subscribed retained settings topic={=str}/settings/# suback_rtt_ms={=u64} quiet_ms={=u64}",
                                    mm2.prefix.as_str(),
                                    suback_rtt.as_millis(),
                                    quiet.as_millis()
                                );
                                *self = Self::Drain {
                                    deadline: now.saturating_add(quiet),
                                    quiet,
                                };
                                continue;
                            }
                            OpStatus::Invalidated => {
                                return Err(Error::Mqtt(MqttError::Disconnected));
                            }
                        }
                    }

                    match subscribe_settings(&mm2.prefix, session).await {
                        Ok(next) => {
                            *start = Instant::now();
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_retryable_startup_error(&err) => {
                            let _ = session.poll().await?;
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
                Self::Drain { deadline, quiet } => {
                    match with_deadline(*deadline, session.poll()).await {
                        Ok(Ok(Some(inbound))) => {
                            if apply_retained(mm2.prefix.as_str(), settings, &inbound) {
                                // Retained storage has no commit marker. Resetting to the last accepted
                                // retained publish keeps the heuristic simple and deterministic.
                                *deadline = Instant::now().saturating_add(*quiet);
                            }
                            return Ok(false);
                        }
                        Ok(Ok(None)) => return Ok(false),
                        Ok(Err(err)) => return Err(Error::Mqtt(err)),
                        Err(_) => {
                            *self = Self::Unsubscribe(None);
                            continue;
                        }
                    }
                }
                Self::Unsubscribe(op) => match super::poll_op(session, op)? {
                    PendingOp::Pending => {
                        let _ = session.poll().await?;
                        return Ok(false);
                    }
                    PendingOp::Complete => {
                        crate::info!("Completed retained settings load");
                        *self = Self::Done;
                        return Ok(true);
                    }
                    PendingOp::Idle => match unsubscribe_settings(&mm2.prefix, session).await {
                        Ok(next) => {
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_retryable_startup_error(&err) => {
                            let _ = session.poll().await?;
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    },
                },
                Self::Done => return Ok(true),
            }
        }
    }
}

fn retained_quiet_window(suback_rtt: Duration) -> Duration {
    let relative = suback_rtt.checked_mul(3).unwrap_or(Duration::MAX);
    Duration::from_millis(100)
        .checked_add(relative)
        .unwrap_or(Duration::MAX)
}

fn apply_retained<Settings>(
    prefix: &str,
    settings: &mut Settings,
    inbound: &InboundPublish<'_>,
) -> bool
where
    Settings: TreeSchema + TreeDeserializeOwned,
{
    // Startup recovery only trusts previous authoritative mirror publications. No-rev settings are
    // reserved for the runtime compatibility path and stale topics are left for smarter clients to
    // prune because the device cannot enumerate broker-retained state outside this subscription.
    if inbound.retain() != Retain::Retained || inbound.payload().is_empty() {
        return false;
    }

    let Some(path) = settings_path(inbound.topic(), prefix) else {
        return false;
    };

    match rev(inbound) {
        Rev::Valid => {}
        Rev::Absent => {
            crate::debug!(
                "Ignoring retained setting without rev topic={=str}",
                inbound.topic()
            );
            return false;
        }
        Rev::Invalid => {
            crate::debug!(
                "Ignoring retained setting with invalid rev topic={=str}",
                inbound.topic()
            );
            return false;
        }
    }

    let mut state = [0; crate::MAX_DEPTH];
    let Some(depth) = resolve_leaf::<Settings>(path, &mut state) else {
        crate::debug!(
            "Ignoring stale retained setting topic={=str}",
            inbound.topic()
        );
        return false;
    };

    match set_leaf(settings, &state[..depth], inbound.payload()) {
        Ok(()) => {
            crate::debug!(
                "Loaded retained setting topic={=str} depth={=usize} payload_len={=usize}",
                inbound.topic(),
                depth,
                inbound.payload().len()
            );
            true
        }
        Err(err) => {
            crate::debug!(
                "Dropping invalid retained setting topic={=str} depth={=usize} payload_len={=usize} class={=str}",
                inbound.topic(),
                err.depth,
                inbound.payload().len(),
                match &err.inner {
                    SerdeError::Value(_) => "Value",
                    SerdeError::Inner(_) => "Deserialize",
                    SerdeError::Finalization(_) => "Finalization",
                }
            );
            false
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
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        loop {
            match self {
                Self::Schema { sync, op } => {
                    if step_schema::<Settings, _>(&mm2.prefix, session, sync, op).await? {
                        mm2.manifest.schema_pages = sync.page;
                        mm2.manifest.schema_rev = sync.hash;
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

async fn step_schema<Settings, IO>(
    prefix: &TopicString,
    session: &mut Session<'_, IO>,
    sync: &mut SchemaSync,
    op: &mut Option<Op>,
) -> Result<bool, Error<IO::Error>>
where
    Settings: TreeSerialize,
    IO: Io,
{
    match super::poll_op(session, op)? {
        PendingOp::Pending => return Ok(false),
        PendingOp::Complete | PendingOp::Idle => {}
    }

    if sync.next == sync.defs.len() {
        crate::info!(
            "Completed schema sync pages={=usize} rev={=u32}",
            sync.page,
            sync.hash
        );
        return Ok(true);
    }

    crate::debug!(
        "Publishing schema page={=usize} next_def={=usize} defs_total={=usize}",
        sync.page,
        sync.next,
        sync.defs.len()
    );
    let topic = schema_page_topic(prefix, sync.page);
    let mut advanced = None::<(usize, u32)>;
    let publication: Publication<'_, PublishPayload<'_, '_, Settings>> = Publication::new(
        &topic,
        PublishPayload::SchemaPage {
            defs: &sync.defs,
            next: sync.next,
            hash: sync.hash,
            advanced: &mut advanced,
        },
    )
    .properties(crate::RETAINED_TEXT_PROPERTIES)
    .qos(QoS::AtLeastOnce)
    .retain();
    match session.publish(publication).await {
        Ok(next_op) => {
            let Some((count, hash)) = advanced else {
                return Err(Error::Mqtt(ResourceError::BufferTooSmall.into()));
            };
            sync.next += count;
            sync.page += 1;
            sync.hash = hash;
            *op = next_op;
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

pub(crate) async fn step_publisher<Settings, IO>(
    publisher: &mut Publisher,
    mm2: &mut Miniconf<Settings>,
    session: &mut Session<'_, IO>,
    settings: &Settings,
) -> Result<bool, Error<IO::Error>>
where
    Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
    IO: Io,
{
    loop {
        if publisher.iter.is_none() {
            publisher.iter = Some(SettingsSync::with_root(
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
    let topics = [TopicFilter::new(&topic).options(
        SubscriptionOptions::default()
            .maximum_qos(QoS::AtLeastOnce)
            .ignore_local_messages(),
    )];
    session.subscribe(&topics, &[]).await.map_err(Into::into)
}

async fn subscribe_settings<IO>(
    prefix: &TopicString,
    session: &mut Session<'_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/settings/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    crate::debug!("Subscribing retained settings topic={=str}", topic.as_str());
    let topics = [TopicFilter::new(&topic).options(
        SubscriptionOptions::default()
            .retain_behavior(RetainHandling::Immediately)
            .maximum_qos(QoS::AtLeastOnce)
            .retain_as_published()
            .ignore_local_messages(),
    )];
    session.subscribe(&topics, &[]).await.map_err(Into::into)
}

async fn unsubscribe_settings<IO>(
    prefix: &TopicString,
    session: &mut Session<'_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/settings/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    crate::debug!(
        "Unsubscribing retained settings topic={=str}",
        topic.as_str()
    );
    session
        .unsubscribe(&[topic.as_str()], &[])
        .await
        .map_err(Into::into)
}
