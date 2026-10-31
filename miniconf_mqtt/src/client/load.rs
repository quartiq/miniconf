use embassy_time::{Duration, Instant, with_deadline};
use miniconf::{SerdeError, TreeDeserializeOwned};
use minimq::{
    Connection, Error as MqttError, InboundPublish, Io, Op, QoS, ResourceError, RetainHandling,
    SubscriptionOptions, TopicFilter,
};

use super::request::{Auth, auth, resolve_leaf, set_leaf};
use super::{Miniconf, PendingOp, is_backpressure, poll_op};
use crate::{Error, MAX_DEPTH, TopicString, debug, info, message::settings_path};

/// Cold-boot retained-settings load workflow.
///
/// Run this after the device process boots and connects, before its first connected [`Startup`](crate::Startup),
/// when retained `settings/#` should seed the live settings tree.
///
/// Do not run this on network reconnects of a still-running device. Local settings are already the
/// authority in that case, even when the broker reports `ConnectEvent::SessionCreated` because the MQTT
/// connection was not resumed.
#[must_use = "drive retained loading to completion before starting Miniconf publication"]
pub struct LoadRetained {
    phase: LoadRetainedPhase,
}

enum LoadRetainedPhase {
    Subscribe {
        start: Instant,
        op: Option<super::PendingOperation>,
    },
    Drain {
        deadline: Instant,
        quiet: Duration,
    },
    Unsubscribe(Option<super::PendingOperation>),
    Done,
}

impl Default for LoadRetained {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadRetained {
    /// Record a completion, returning whether it belongs to this workflow.
    /// The next `step` reports any broker rejection.
    pub fn handle_completion(&mut self, op: Op, outcome: &minimq::Outcome<'_>) -> bool {
        match &mut self.phase {
            LoadRetainedPhase::Subscribe { op: pending, .. }
            | LoadRetainedPhase::Unsubscribe(pending) => super::complete_op(pending, op, outcome),
            _ => false,
        }
    }

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
    /// before the first Miniconf startup, then use [`Startup::connected`](crate::Startup::connected) to publish the recovered
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
        if connection.session().protocol() != minimq::Protocol::V5 {
            return Err(MqttError::InvalidRequest.into());
        }
        loop {
            match &mut self.phase {
                LoadRetainedPhase::Subscribe { start, op } => {
                    match poll_op(connection, op)? {
                        PendingOp::Pending => {
                            match connection.poll().await? {
                                minimq::Event::Publish(inbound) => {
                                    apply_retained(miniconf.prefix.as_str(), settings, &inbound);
                                }
                                minimq::Event::Completed {
                                    op: completed,
                                    outcome,
                                } => {
                                    super::complete_op(op, completed, &outcome);
                                }
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
                            *op = Some(next.into());
                            return Ok(false);
                        }
                        Err(err) if is_backpressure(connection, &err) => {
                            if let minimq::Event::Completed { op, outcome } =
                                connection.poll().await?
                            {
                                self.handle_completion(op, &outcome);
                            }
                            return Ok(false);
                        }
                        Err(err) => return Err(err),
                    }
                }
                LoadRetainedPhase::Drain { deadline, quiet } => {
                    match with_deadline(*deadline, connection.poll()).await {
                        Ok(Ok(minimq::Event::Publish(inbound))) => {
                            if apply_retained(miniconf.prefix.as_str(), settings, &inbound) {
                                // Retained storage has no commit marker. Resetting to the last accepted
                                // retained publish keeps the heuristic simple and deterministic.
                                *deadline = Instant::now().saturating_add(*quiet);
                            }
                            return Ok(false);
                        }
                        Ok(Ok(minimq::Event::Completed { .. })) => return Ok(false),
                        Ok(Err(err)) => return Err(Error::Mqtt(err)),
                        Err(_) => {
                            self.phase = LoadRetainedPhase::Unsubscribe(None);
                            continue;
                        }
                    }
                }
                LoadRetainedPhase::Unsubscribe(op) => match poll_op(connection, op)? {
                    PendingOp::Pending => {
                        if let minimq::Event::Completed { op, outcome } = connection.poll().await? {
                            self.handle_completion(op, &outcome);
                        }
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
                                *op = Some(next.into());
                                return Ok(false);
                            }
                            Err(err) if is_backpressure(connection, &err) => {
                                if let minimq::Event::Completed { op, outcome } =
                                    connection.poll().await?
                                {
                                    self.handle_completion(op, &outcome);
                                }
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
