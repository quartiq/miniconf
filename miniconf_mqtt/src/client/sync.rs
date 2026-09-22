use embassy_time::{Duration, Instant, with_deadline};
use miniconf::{SerdeError, TreeDeserializeOwned, TreeSchema, TreeSerialize};
use minimq::{
    Connection, Error as MqttError, InboundPublish, Io, Op, PubError, Publication, QoS,
    ResourceError, RetainHandling, SubscriptionOptions, TopicFilter,
};

use super::poll_op;
use super::request::{Auth, auth, resolve_leaf, set_leaf};
use crate::{
    Error, MAX_DEPTH, RETAINED_TEXT_PROPERTIES, TopicString,
    client::{
        ChangedKey, Miniconf, PayloadError, PendingOp, PublishPayload, Publisher,
        publish_alive_once, schema_page_topic,
    },
    debug, info,
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
        miniconf: &mut Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &mut Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        loop {
            match self {
                Self::Subscribe { start, op } => {
                    match poll_op(connection, op)? {
                        PendingOp::Pending => {
                            if let Some(inbound) = connection.poll().await? {
                                apply_retained(miniconf.prefix.as_str(), settings, &inbound);
                            }
                            return Ok(false);
                        }
                        PendingOp::Complete => {
                            let now = Instant::now();
                            let suback_rtt = now.saturating_duration_since(*start);
                            let quiet = retained_quiet_window(suback_rtt);
                            debug!(
                                "Subscribed retained settings topic={=str}/settings/# suback_rtt_ms={=u64} quiet_ms={=u64}",
                                miniconf.prefix.as_str(),
                                suback_rtt.as_millis(),
                                quiet.as_millis()
                            );
                            *self = Self::Drain {
                                deadline: now.saturating_add(quiet),
                                quiet,
                            };
                            continue;
                        }
                        PendingOp::Idle => {}
                    }

                    match subscribe_settings(&miniconf.prefix, connection).await {
                        Ok(next) => {
                            *start = Instant::now();
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_retryable_startup_error(&err) => {
                            let _ = connection.poll().await?;
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
                Self::Drain { deadline, quiet } => {
                    match with_deadline(*deadline, connection.poll()).await {
                        Ok(Ok(Some(inbound))) => {
                            if apply_retained(miniconf.prefix.as_str(), settings, &inbound) {
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
                Self::Unsubscribe(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => {
                        let _ = connection.poll().await?;
                        return Ok(false);
                    }
                    PendingOp::Complete => {
                        info!("Completed retained settings load");
                        *self = Self::Done;
                        return Ok(true);
                    }
                    PendingOp::Idle => {
                        match unsubscribe_settings(&miniconf.prefix, connection).await {
                            Ok(next) => {
                                *op = Some(next);
                                return Ok(false);
                            }
                            Err(err) if is_retryable_startup_error(&err) => {
                                let _ = connection.poll().await?;
                                return Ok(false);
                            }
                            Err(err) => return Err(err),
                        }
                    }
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
    // Startup recovery only trusts previous authoritative mirror publications. No-auth settings are
    // reserved for the runtime compatibility path and stale topics are left for smarter clients to
    // prune because the device cannot enumerate broker-retained state outside this subscription.
    if !inbound.retained() || inbound.payload().is_empty() {
        return false;
    }

    let Some(path) = settings_path(inbound.topic(), prefix) else {
        return false;
    };

    match auth(inbound) {
        Auth::Valid => {}
        Auth::Absent => {
            debug!(
                "Ignoring retained setting without auth topic={=str}",
                inbound.topic()
            );
            return false;
        }
        Auth::Invalid => {
            debug!(
                "Ignoring retained setting with invalid auth topic={=str}",
                inbound.topic()
            );
            return false;
        }
    }

    let mut state = [0; MAX_DEPTH];
    let Some(depth) = resolve_leaf::<Settings>(path, &mut state) else {
        debug!(
            "Ignoring stale retained setting topic={=str}",
            inbound.topic()
        );
        return false;
    };

    match set_leaf(settings, &state[..depth], inbound.payload()) {
        Ok(()) => {
            debug!(
                "Loaded retained setting topic={=str} depth={=usize} payload_len={=usize}",
                inbound.topic(),
                depth,
                inbound.payload().len()
            );
            true
        }
        Err(err) => {
            debug!(
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
        miniconf: &mut Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSchema + TreeSerialize + TreeDeserializeOwned,
        IO: Io,
    {
        // Replayed traffic must release its storage before startup uses the publish budget.
        if !matches!(self, Self::Done) && !connection.session().is_publish_quiescent() {
            return Ok(false);
        }
        loop {
            match self {
                Self::Schema { sync, op } => {
                    if step_schema::<Settings, _>(&miniconf.prefix, connection, sync, op).await? {
                        miniconf.manifest.schema_pages = sync.page;
                        miniconf.manifest.schema_rev = sync.hash;
                        debug!(
                            "Schema startup phase complete pages={=usize} rev={=u32}",
                            miniconf.manifest.schema_pages, miniconf.manifest.schema_rev
                        );
                        *self = Self::Settings(Publisher::root(Settings::SCHEMA));
                        continue;
                    }
                    return Ok(false);
                }
                Self::Settings(publisher) => {
                    if publisher.step(miniconf, connection, settings).await? {
                        debug!("Settings startup phase complete");
                        *self = Self::SubscribeSet(None);
                        continue;
                    }
                    return Ok(false);
                }
                Self::SubscribeSet(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        debug!("Subscribed Miniconf request ingress");
                        *self = Self::Alive(None);
                        continue;
                    }
                    PendingOp::Idle => match subscribe_set(&miniconf.prefix, connection).await {
                        Ok(next) => {
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_retryable_startup_error(&err) => return Ok(false),
                        Err(err) => return Err(err),
                    },
                },
                Self::Alive(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        info!(
                            "Completed Miniconf startup epoch={=u32} schema_rev={=u32}",
                            miniconf.manifest.epoch, miniconf.manifest.schema_rev
                        );
                        miniconf.startup_complete = true;
                        *self = Self::Done;
                        return Ok(true);
                    }
                    PendingOp::Idle => {
                        match publish_alive_once::<Settings, _>(
                            &miniconf.prefix,
                            &miniconf.manifest,
                            connection,
                        )
                        .await
                        {
                            Ok(next) => {
                                *op = Some(next);
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
    connection: &mut Connection<'_, '_, IO>,
    sync: &mut SchemaSync,
    op: &mut Option<Op>,
) -> Result<bool, Error<IO::Error>>
where
    Settings: TreeSerialize,
    IO: Io,
{
    match poll_op(connection, op)? {
        PendingOp::Pending => return Ok(false),
        PendingOp::Complete | PendingOp::Idle => {}
    }

    if sync.next == sync.defs.len() {
        info!(
            "Completed schema sync pages={=usize} rev={=u32}",
            sync.page, sync.hash
        );
        return Ok(true);
    }

    debug!(
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
    .properties(RETAINED_TEXT_PROPERTIES)
    .qos(QoS::AtLeastOnce)
    .retain();
    match connection.publish(publication).await {
        Ok(Some(next_op)) => {
            let Some((count, hash)) = advanced else {
                return Err(Error::Mqtt(ResourceError::BufferTooSmall.into()));
            };
            sync.next += count;
            sync.page += 1;
            sync.hash = hash;
            *op = Some(next_op);
            Ok(false)
        }
        Ok(None) => Err(Error::Mqtt(MqttError::InvalidRequest)),
        Err(PubError::Session(MqttError::NotReady))
        | Err(PubError::Session(MqttError::Resource(ResourceError::InflightExhausted))) => {
            Ok(false)
        }
        Err(PubError::Payload((true, PayloadError::Schema(id)))) => {
            info!(
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
    miniconf: &mut Miniconf<Settings>,
    connection: &mut Connection<'_, '_, IO>,
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
            info!(
                "Starting retained settings sync root_depth={=usize}",
                publisher.root.as_ref().len()
            );
        }

        let state = match publisher.pending {
            Some(state) => state,
            None => {
                let iter = publisher.iter.as_mut().unwrap();
                let Some(Ok(())) = iter.next() else {
                    info!("Completed retained settings sync");
                    return Ok(true);
                };
                let full = iter
                    .indices()
                    .ok_or_else(|| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
                let mut state = [0; MAX_DEPTH];
                state[..full.len()].copy_from_slice(full);
                let state = ChangedKey::new(state, full.len());
                publisher.pending = Some(state);
                debug!(
                    "Preparing retained setting publication depth={=usize}",
                    state.as_ref().len()
                );
                state
            }
        };

        match poll_op(connection, &mut publisher.op)? {
            PendingOp::Pending => return Ok(false),
            PendingOp::Complete => {
                debug!(
                    "Published retained setting depth={=usize}",
                    state.as_ref().len()
                );
                publisher.pending = None;
                continue;
            }
            PendingOp::Idle => {}
        }

        match miniconf
            .publish_current(connection, settings, state.as_ref())
            .await
        {
            Ok(op) => {
                publisher.op = Some(op);
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
    connection: &mut Connection<'_, '_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/set/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    debug!(
        "Subscribing Miniconf request ingress topic={=str}",
        topic.as_str()
    );
    let topics = [TopicFilter::new(&topic).options(
        SubscriptionOptions::default()
            .maximum_qos(QoS::AtLeastOnce)
            .ignore_local_messages(),
    )];
    connection.subscribe(&topics, &[]).await.map_err(Into::into)
}

async fn subscribe_settings<IO>(
    prefix: &TopicString,
    connection: &mut Connection<'_, '_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/settings/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    debug!("Subscribing retained settings topic={=str}", topic.as_str());
    let topics = [TopicFilter::new(&topic).options(
        SubscriptionOptions::default()
            .retain_behavior(RetainHandling::Immediately)
            .maximum_qos(QoS::AtLeastOnce)
            .retain_as_published()
            .ignore_local_messages(),
    )];
    connection.subscribe(&topics, &[]).await.map_err(Into::into)
}

async fn unsubscribe_settings<IO>(
    prefix: &TopicString,
    connection: &mut Connection<'_, '_, IO>,
) -> Result<Op, Error<IO::Error>>
where
    IO: Io,
{
    let mut topic = prefix.clone();
    topic
        .push_str("/settings/#")
        .map_err(|_| Error::Mqtt(ResourceError::BufferTooSmall.into()))?;
    debug!(
        "Unsubscribing retained settings topic={=str}",
        topic.as_str()
    );
    connection
        .unsubscribe(&[topic.as_str()], &[])
        .await
        .map_err(Into::into)
}
