use miniconf::{
    IntoKeys, NodeIter, ResolveError, Schema, TreeSchema, TreeSerialize, compact_schema::SchemaDefs,
};
use minimq::{
    ConnectEvent, Connection, Error as MqttError, Event, Io, Op, Outcome, Protocol, PubError,
    Publication, QoS, ResourceError, RetainHandling, SubscriptionOptions, TopicFilter,
};

use super::{is_backpressure, poll_op};
use crate::{
    Error, MAX_DEPTH, MAX_SCHEMA_DEFS, RETAINED_TEXT_PROPERTIES, TopicString,
    client::{
        ChangedKey, Miniconf, PayloadError, PendingOp, PublishPayload, publish_alive_once,
        schema_page_topic,
    },
    debug, info,
};

/// Miniconf startup workflow for one MQTT connection event.
#[must_use = "drive startup to completion before relying on Miniconf startup state"]
pub struct Startup {
    phase: StartupPhase,
}

#[allow(clippy::large_enum_variant)]
enum StartupPhase {
    Schema {
        sync: SchemaSync,
        op: Option<super::PendingOperation>,
    },
    Settings(Publisher),
    SubscribeSet(Option<super::PendingOperation>),
    Alive(Option<super::PendingOperation>),
    Done,
}

/// Explicit retained publication workflow for a leaf, subtree, or root.
#[must_use = "drive the publisher to completion to update the retained subtree"]
pub struct Publisher {
    schema: &'static Schema,
    root: ChangedKey,
    iter: Option<NodeIter<(), MAX_DEPTH>>,
    pending: Option<ChangedKey>,
    op: Option<super::PendingOperation>,
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
    /// Record a completion, returning whether it belongs to this workflow.
    /// The next `step` reports any broker rejection.
    pub fn handle_completion(&mut self, op: Op, outcome: &Outcome<'_>) -> bool {
        match &mut self.phase {
            StartupPhase::Schema { op: pending, .. }
            | StartupPhase::SubscribeSet(pending)
            | StartupPhase::Alive(pending) => super::complete_op(pending, op, outcome),
            StartupPhase::Settings(publisher) => publisher.handle_completion(op, outcome),
            StartupPhase::Done => false,
        }
    }

    /// Begin Miniconf startup after one MQTT connect event.
    pub fn new<Settings>(miniconf: &mut Miniconf<Settings>, event: ConnectEvent) -> Self
    where
        Settings: TreeSchema,
    {
        match event {
            ConnectEvent::SessionResumed if miniconf.startup_complete => {
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

    /// Begin the Miniconf startup path for `ConnectEvent::SessionCreated`.
    ///
    /// Use this after cold-boot [`LoadRetained`](crate::LoadRetained) so the retained mirror is republished
    /// authoritatively from the recovered local settings.
    /// Also use this after losing service follow-ups, even if MQTT resumed its session:
    /// queued mirrors not yet admitted to MQTT cannot be recovered by packet replay.
    /// Full startup repairs the mirror from live settings, but does not recreate lost replies.
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
    /// This helper owns connection progress until startup completes. Use `step()` and
    /// route events when other workflows share the connection.
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
            if let Event::Completed { op, outcome } = connection.poll().await? {
                self.handle_completion(op, &outcome);
            }
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
        if connection.session().protocol() != Protocol::V5 {
            return Err(MqttError::InvalidRequest.into());
        }
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
                            *op = Some(next.into());
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
                                *op = Some(next.into());
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

impl Publisher {
    /// Record a completion, returning whether it belongs to this workflow.
    /// The next `step` reports any broker rejection.
    pub fn handle_completion(&mut self, op: Op, outcome: &Outcome<'_>) -> bool {
        super::complete_op(&mut self.op, op, outcome)
    }

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
    /// This helper owns connection progress until publication completes. Use [`Publisher::step`]
    /// and route events when other workflows share the connection.
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
            if let Event::Completed { op, outcome } = connection.poll().await? {
                self.handle_completion(op, &outcome);
            }
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
        if connection.session().protocol() != Protocol::V5 {
            return Err(MqttError::InvalidRequest.into());
        }
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
                    self.op = Some(op.into());
                    return Ok(false);
                }
                Err(err) if is_backpressure(connection, &err) => return Ok(false),
                Err(err) => return Err(err),
            }
        }
    }
}

async fn step_schema<Settings, IO>(
    prefix: &TopicString,
    connection: &mut Connection<'_, '_, IO>,
    sync: &mut SchemaSync,
    op: &mut Option<super::PendingOperation>,
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
            *op = Some(next_op.into());
            Ok(false)
        }
        Ok(None) => Err(Error::Mqtt(MqttError::InvalidRequest)),
        Err(PubError::Session(MqttError::NotReady))
        | Err(PubError::Session(MqttError::Resource(ResourceError::InflightExhausted))) => {
            Ok(false)
        }
        Err(PubError::Payload(PayloadError::Schema(id))) => {
            debug!(
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
