use embassy_time::{Duration, Instant, with_deadline};
use miniconf::{
    IntoKeys, NodeIter, ResolveError, Schema, SerdeError, TreeDeserializeOwned, TreeSchema,
    TreeSerialize, compact_schema::SchemaDefs,
};
use minimq::{
    ConnectEvent, Connection, Error as MqttError, InboundPublish, Io, Op, PubError, Publication,
    QoS, ResourceError, RetainHandling, SubscriptionOptions, TopicFilter,
};

use super::request::{Auth, auth, resolve_leaf, set_leaf};
use super::{is_backpressure, poll_op};
use crate::{
    Error, MAX_DEPTH, MAX_SCHEMA_DEFS, RETAINED_TEXT_PROPERTIES, TopicString,
    client::{
        ChangedKey, Miniconf, PayloadError, PendingOp, PublishPayload, publish_alive_once,
        schema_page_topic,
    },
    debug, info,
    message::settings_path,
};

/// Miniconf startup workflow for one MQTT connection event.
#[must_use = "drive startup to completion before relying on Miniconf startup state"]
pub struct Startup {
    phase: StartupPhase,
}

#[allow(clippy::large_enum_variant)]
enum StartupPhase {
    Schema { sync: SchemaSync, op: Option<Op> },
    Settings(Publisher),
    SubscribeSet(Option<Op>),
    Alive(Option<Op>),
    Done,
}

/// Cold-boot retained-settings load workflow.
///
/// Run this after the device process boots and connects, before its first connected [`Startup`],
/// when retained `settings/#` should seed the live settings tree.
///
/// Do not run this on network reconnects of a still-running device. Local settings are already the
/// authority in that case, even when the broker reports `ConnectEvent::Connected` because the MQTT
/// connection was not resumed.
#[must_use = "drive retained loading to completion before starting Miniconf publication"]
pub struct LoadRetained {
    phase: LoadRetainedPhase,
}

enum LoadRetainedPhase {
    Subscribe { start: Instant, op: Option<Op> },
    Drain { deadline: Instant, quiet: Duration },
    Unsubscribe(Option<Op>),
    Done,
}

/// Explicit retained publication workflow for a leaf, subtree, or root.
#[must_use = "drive the publisher to completion to update the retained subtree"]
pub struct Publisher {
    schema: &'static Schema,
    root: ChangedKey,
    iter: Option<NodeIter<(), MAX_DEPTH>>,
    pending: Option<ChangedKey>,
    op: Option<Op>,
}

struct SchemaSync {
    defs: SchemaDefs<MAX_SCHEMA_DEFS>,
    next: usize,
    page: usize,
    hash: u32,
}

impl SchemaSync {
    fn new(schema: &'static Schema) -> Self {
        Self {
            defs: SchemaDefs::new(schema).unwrap(),
            next: 0,
            page: 0,
            hash: <u32 as yafnv::Fnv>::OFFSET_BASIS,
        }
    }
}

impl Startup {
    /// Begin Miniconf startup after one MQTT connect event.
    pub fn new<Settings>(miniconf: &mut Miniconf<Settings>, event: ConnectEvent) -> Self
    where
        Settings: TreeSchema,
    {
        match event {
            ConnectEvent::Reconnected if miniconf.startup_complete => {
                info!(
                    "Starting reconnected Miniconf startup prefix={=str} epoch={=u32} schema_rev={=u32}",
                    miniconf.prefix.as_str(),
                    miniconf.manifest.epoch,
                    miniconf.manifest.schema_rev
                );
                Self {
                    phase: StartupPhase::Alive(None),
                }
            }
            _ => Self::connected(miniconf),
        }
    }

    /// Begin the Miniconf startup path for `ConnectEvent::Connected`.
    ///
    /// Use this after cold-boot [`LoadRetained`] so the retained mirror is republished
    /// authoritatively from the recovered local settings.
    pub fn connected<Settings>(miniconf: &mut Miniconf<Settings>) -> Self
    where
        Settings: TreeSchema,
    {
        miniconf.startup_complete = false;
        miniconf.manifest.epoch = miniconf.manifest.epoch.wrapping_add(1);
        miniconf.manifest.schema_rev = 0;
        miniconf.manifest.schema_pages = 0;
        info!(
            "Starting connected Miniconf startup prefix={=str} epoch={=u32}",
            miniconf.prefix.as_str(),
            miniconf.manifest.epoch
        );
        Self {
            phase: StartupPhase::Schema {
                sync: SchemaSync::new(Settings::SCHEMA),
                op: None,
            },
        }
    }

    /// Run Miniconf startup to completion.
    ///
    /// This helper may consume and discard surfaced inbound publishes while bootstrapping.
    pub async fn run<Settings, IO>(
        &mut self,
        miniconf: &mut Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        while !self.step(miniconf, connection, settings).await? {
            let _ = connection.poll().await?;
        }
        Ok(())
    }

    /// Advance Miniconf startup.
    ///
    /// `Ok(true)` means startup is complete.
    ///
    /// `Ok(false)` means no more immediate startup progress is possible. Wait for later connection
    /// progress, then call `step()` again.
    ///
    /// This method never consumes inbound publishes. The caller drives connection progress.
    pub async fn step<Settings, IO>(
        &mut self,
        miniconf: &mut Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        // Replayed traffic must release its storage before startup uses the publish budget.
        if !matches!(self.phase, StartupPhase::Done) && !connection.session().is_publish_quiescent()
        {
            return Ok(false);
        }
        loop {
            match &mut self.phase {
                StartupPhase::Schema { sync, op } => {
                    if step_schema::<Settings, _>(&miniconf.prefix, connection, sync, op).await? {
                        miniconf.manifest.schema_pages = sync.page;
                        miniconf.manifest.schema_rev = sync.hash;
                        debug!(
                            "Schema startup phase complete pages={=usize} rev={=u32}",
                            miniconf.manifest.schema_pages, miniconf.manifest.schema_rev
                        );
                        self.phase = StartupPhase::Settings(Publisher::root(Settings::SCHEMA));
                        continue;
                    }
                    return Ok(false);
                }
                StartupPhase::Settings(publisher) => {
                    if publisher.step(miniconf, connection, settings).await? {
                        debug!("Settings startup phase complete");
                        self.phase = StartupPhase::SubscribeSet(None);
                        continue;
                    }
                    return Ok(false);
                }
                StartupPhase::SubscribeSet(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        debug!("Subscribed Miniconf request ingress");
                        self.phase = StartupPhase::Alive(None);
                        continue;
                    }
                    PendingOp::Idle => match subscribe_set(&miniconf.prefix, connection).await {
                        Ok(next) => {
                            *op = Some(next);
                            return Ok(false);
                        }
                        Err(err) if is_backpressure(connection, &err) => return Ok(false),
                        Err(err) => return Err(err),
                    },
                },
                StartupPhase::Alive(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => return Ok(false),
                    PendingOp::Complete => {
                        info!(
                            "Completed Miniconf startup epoch={=u32} schema_rev={=u32}",
                            miniconf.manifest.epoch, miniconf.manifest.schema_rev
                        );
                        miniconf.startup_complete = true;
                        self.phase = StartupPhase::Done;
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
                            Err(err) if is_backpressure(connection, &err) => return Ok(false),
                            Err(err) => return Err(err),
                        }
                    }
                },
                StartupPhase::Done => return Ok(true),
            }
        }
    }
}

impl Default for LoadRetained {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadRetained {
    /// Begin loading retained `settings/#` into local settings.
    pub fn new() -> Self {
        Self {
            phase: LoadRetainedPhase::Subscribe {
                start: Instant::now(),
                op: None,
            },
        }
    }

    /// Run retained settings load to completion.
    ///
    /// This helper owns a temporary `settings/#` subscription. For cold-boot recovery, run it
    /// before the first Miniconf startup, then use [`Startup::connected`] to publish the recovered
    /// settings authoritatively.
    pub async fn run<Settings, IO>(
        &mut self,
        miniconf: &Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &mut Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeDeserializeOwned,
        IO: Io,
    {
        while !self.step(miniconf, connection, settings).await? {}
        Ok(())
    }

    /// Advance retained settings loading.
    ///
    /// `Ok(true)` means retained loading is complete and the temporary subscription has been
    /// removed.
    ///
    /// This workflow consumes inbound publishes while draining the retained `settings/#` burst.
    pub async fn step<Settings, IO>(
        &mut self,
        miniconf: &Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &mut Settings,
    ) -> Result<bool, Error<IO::Error>>
    where
        Settings: TreeDeserializeOwned,
        IO: Io,
    {
        loop {
            match &mut self.phase {
                LoadRetainedPhase::Subscribe { start, op } => {
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
                            self.phase = LoadRetainedPhase::Drain {
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
                        Err(err) if is_backpressure(connection, &err) => {
                            let _ = connection.poll().await?;
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
                LoadRetainedPhase::Drain { deadline, quiet } => {
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
                            self.phase = LoadRetainedPhase::Unsubscribe(None);
                            continue;
                        }
                    }
                }
                LoadRetainedPhase::Unsubscribe(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => {
                        let _ = connection.poll().await?;
                        return Ok(false);
                    }
                    PendingOp::Complete => {
                        info!("Completed retained settings load");
                        self.phase = LoadRetainedPhase::Done;
                        return Ok(true);
                    }
                    PendingOp::Idle => {
                        match unsubscribe_settings(&miniconf.prefix, connection).await {
                            Ok(next) => {
                                *op = Some(next);
                                return Ok(false);
                            }
                            Err(err) if is_backpressure(connection, &err) => {
                                let _ = connection.poll().await?;
                                return Ok(false);
                            }
                            Err(err) => return Err(err),
                        }
                    }
                },
                LoadRetainedPhase::Done => return Ok(true),
            }
        }
    }
}

impl Publisher {
    /// Begin explicit retained publication for the whole tree root.
    pub fn root(schema: &'static Schema) -> Self {
        Self {
            schema,
            root: ChangedKey::new([0; MAX_DEPTH], 0),
            iter: None,
            pending: None,
            op: None,
        }
    }

    /// Begin explicit retained publication for one leaf or subtree.
    pub fn by_key(schema: &'static Schema, key: impl IntoKeys) -> Result<Self, ResolveError> {
        let mut state = [0; MAX_DEPTH];
        let lookup = schema.resolve_into(key, &mut state)?;
        Ok(Self {
            schema,
            root: ChangedKey::new(state, lookup.depth),
            iter: None,
            pending: None,
            op: None,
        })
    }

    /// Run retained subtree publication to completion.
    ///
    /// This is the simple unbounded helper. It may discard unrelated inbound publishes while
    /// waiting for MQTT connection progress. Use [`Publisher::step`] when those publishes must be
    /// routed elsewhere.
    pub async fn run<Settings, IO>(
        &mut self,
        miniconf: &Miniconf<Settings>,
        connection: &mut Connection<'_, '_, IO>,
        settings: &Settings,
    ) -> Result<(), Error<IO::Error>>
    where
        Settings: TreeSerialize,
        IO: Io,
    {
        while !self.step(miniconf, connection, settings).await? {
            let _ = connection.poll().await?;
        }
        Ok(())
    }

    /// Advance retained subtree publication.
    ///
    /// `Ok(true)` means publication is complete.
    ///
    /// `Ok(false)` means no more immediate publication progress is possible. Wait for later
    /// connection progress, then call `step()` again.
    ///
    /// This method never consumes unrelated inbound publishes.
    pub async fn step<Settings, IO>(
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
            if self.iter.is_none() {
                self.iter = Some(NodeIter::with_root(self.schema, self.root.as_ref())?);
                info!(
                    "Starting retained settings sync root_depth={=usize}",
                    self.root.as_ref().len()
                );
            }

            let state = match self.pending {
                Some(state) => state,
                None => {
                    let iter = self.iter.as_mut().unwrap();
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
                    self.pending = Some(state);
                    state
                }
            };

            match poll_op(connection, &mut self.op)? {
                PendingOp::Pending => return Ok(false),
                PendingOp::Complete => {
                    self.pending = None;
                    continue;
                }
                PendingOp::Idle => {}
            }

            match miniconf
                .publish_current(connection, settings, state.as_ref())
                .await
            {
                Ok(op) => {
                    self.op = Some(op);
                    return Ok(false);
                }
                Err(err) if is_backpressure(connection, &err) => return Ok(false),
                Err(err) => return Err(err),
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
    Settings: TreeDeserializeOwned,
{
    // Cold-boot recovery accepts only authoritative retained values.
    // No-auth writes belong to runtime compatibility handling; clients prune stale topics.
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
        Err(PubError::Payload(PayloadError::Schema(id))) => {
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
            .retain_behavior(RetainHandling::Never)
            .retain_as_published()
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
