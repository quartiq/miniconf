use core::cell::Cell;
use core::fmt::Write as _;

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Instant;
use heapless::String;
#[cfg(feature = "compat-settings-ingress")]
use log::debug;
use log::{info, warn};
use minimq::{ProtocolError, PubError, Publication, QoS};
use serde::Serialize;

#[cfg(feature = "compat-settings-ingress")]
use super::SettingsIngressPhase;
use super::{Error, MqttClient, Phase};
use crate::{
    EncodeError, MAX_TOPIC_LENGTH,
    message::{DepthError, simple_pub_error},
    schema::serialize_schema_page,
};

impl<'a, Settings, C> MqttClient<'a, Settings, C>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
    C: minimq::transport::Connector,
{
    pub(super) async fn publish_alive(&mut self) -> Result<(), Error<C::Error>> {
        #[derive(Serialize)]
        struct Alive {
            epoch: u32,
            schema_rev: u32,
            pages: usize,
        }

        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/alive")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let body = Alive {
            epoch: self.protocol.manifest.epoch,
            schema_rev: self.protocol.manifest.schema_rev,
            pages: self.protocol.manifest.schema_pages,
        };
        let publication = Publication::new(&topic, |buf: &mut [u8]| {
            serde_json_core::to_slice(&body, buf)
                .map_err(|err| (matches!(err, serde_json_core::ser::Error::BufferFull), err))
        })
        .qos(QoS::AtLeastOnce)
        .retain();
        self.session
            .publish(publication)
            .await
            .map_err(simple_pub_error)
    }

    #[cfg(feature = "compat-settings-ingress")]
    pub(super) fn settings_recovery_wait_deadline(&self) -> Option<Instant> {
        match self.protocol.settings_ingress {
            SettingsIngressPhase::Recovering {
                seen: true,
                deadline: Some(deadline),
            } if matches!(self.protocol.phase, Phase::Idle) => Some(deadline),
            _ => None,
        }
    }

    #[cfg(feature = "compat-settings-ingress")]
    pub(super) fn note_settings_ingress(&mut self) {
        if let SettingsIngressPhase::Recovering { seen, .. } = self.protocol.settings_ingress {
            if !seen {
                debug!("Observed retained settings ingress during recovery");
            }
            self.protocol.settings_ingress = SettingsIngressPhase::Recovering {
                seen: true,
                deadline: Some(Instant::now() + crate::SETTINGS_RECOVERY_QUIESCENCE),
            };
        }
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    pub(super) fn note_settings_ingress(&mut self) {}

    #[cfg(feature = "compat-settings-ingress")]
    pub(super) fn finish_settings_recovery(&mut self, idle: bool) {
        let SettingsIngressPhase::Recovering {
            seen: true,
            deadline: Some(deadline),
        } = self.protocol.settings_ingress
        else {
            return;
        };
        if !idle || Instant::now() < deadline || !matches!(self.protocol.phase, Phase::Idle) {
            return;
        }
        self.protocol.settings_ingress = SettingsIngressPhase::Runtime;
        debug!("Finished settings ingress recovery");
        self.start_settings_sync();
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    pub(super) fn finish_settings_recovery(&mut self, _idle: bool) {}

    pub(super) async fn advance_pending(&mut self, settings: &Settings) {
        if !self.session.can_publish(QoS::AtLeastOnce) {
            return;
        }
        match &self.protocol.phase {
            Phase::Idle => {}
            Phase::Schema(_) => self.advance_schema_pending().await,
            Phase::Settings(_) => self.advance_settings_pending(settings).await,
        }
    }

    async fn advance_schema_pending(&mut self) {
        if !self.session.is_publish_quiescent() {
            return;
        }
        let (defs, next, page, hash) = match &self.protocol.phase {
            Phase::Schema(sync) => (&sync.defs, sync.next, sync.page, sync.hash),
            _ => unreachable!(),
        };
        if next == defs.len() {
            self.finish_schema_sync(page, hash);
            return;
        }

        let topic = self.schema_page_topic(page);
        let advanced = Cell::new(None::<(usize, u32)>);
        let publication = Publication::new(&topic, |buf: &mut [u8]| {
            let page = serialize_schema_page(defs, next, buf).map_err(|id| (true, id))?;
            let next_hash = yafnv::Fnv::fnv1a(hash, buf[..page.len].iter().copied());
            advanced.set(Some((page.count, next_hash)));
            Ok::<usize, EncodeError<usize>>(page.len)
        })
        .qos(QoS::AtLeastOnce)
        .retain();
        if let Err(err) = self.session.publish(publication).await {
            match err {
                PubError::Payload((true, id)) => {
                    warn!("Aborting schema sync after oversized schema entry for definition {id}");
                }
                PubError::Payload((false, _)) => unreachable!(),
                err => {
                    let err = match err {
                        PubError::Session(err) => Error::Mqtt(err),
                        PubError::Payload(_) => unreachable!(),
                    };
                    warn!("Failed to publish schema page {}: {:?}", page, err);
                }
            }
            self.protocol.phase = Phase::Idle;
            self.protocol.followup = Default::default();
            return;
        }

        let Some((count, hash)) = advanced.get() else {
            self.protocol.phase = Phase::Idle;
            self.protocol.followup = Default::default();
            return;
        };
        let Phase::Schema(sync) = &mut self.protocol.phase else {
            unreachable!()
        };
        sync.next += count;
        sync.page += 1;
        sync.hash = hash;
        let finished = sync.next == sync.defs.len();
        let pages = sync.page;
        let hash = sync.hash;
        if finished {
            self.finish_schema_sync(pages, hash);
        }
    }

    fn finish_schema_sync(&mut self, pages: usize, hash: u32) {
        self.protocol.manifest.schema_pages = pages;
        self.protocol.manifest.schema_rev = hash;
        info!(
            "Completed schema sync pages={} rev={}",
            self.protocol.manifest.schema_pages, self.protocol.manifest.schema_rev
        );
        self.protocol.phase = Phase::Idle;
        self.protocol.followup.publish_alive = true;
        self.protocol.followup.publish_all = true;
        self.start_settings_sync();
    }

    async fn advance_settings_pending(&mut self, settings: &Settings) {
        if !self.session.is_publish_quiescent() {
            return;
        }
        let (path, state, depth) = {
            let Phase::Settings(iter) = &mut self.protocol.phase else {
                unreachable!()
            };
            let Some(path) = iter.next() else {
                self.finish_settings_sync().await;
                return;
            };
            let path = match path {
                Ok(path) => path.into_inner(),
                Err(err) => {
                    warn!("Aborting retained settings sync after path iteration failure: {err}");
                    self.protocol.phase = Phase::Idle;
                    self.protocol.followup.publish_alive = false;
                    return;
                }
            };
            let Some(full) = iter.indices() else {
                self.protocol.phase = Phase::Idle;
                self.protocol.followup.publish_alive = false;
                return;
            };
            let mut state = [0; crate::MAX_DEPTH];
            state[..full.len()].copy_from_slice(full);
            (path, state, full.len())
        };

        let topic = self.settings_sync_topic(&path);
        match self.try_publish_leaf(settings, state, depth).await {
            Ok(()) => {}
            Err(PubError::Payload((
                _no_space,
                DepthError {
                    inner:
                        miniconf::SerdeError::Value(
                            miniconf::ValueError::Absent | miniconf::ValueError::Access(_),
                        ),
                    ..
                },
            ))) => {
                if let Err(err) = self.clear_leaf(&topic).await {
                    warn!(
                        "Failed to clear retained setting path={}: {err:?}",
                        path.as_str()
                    );
                    self.protocol.phase = Phase::Idle;
                    self.protocol.followup.publish_alive = false;
                }
            }
            Err(err) => {
                warn!(
                    "Failed to publish retained setting path={}: {:?}",
                    path.as_str(),
                    simple_pub_error(err)
                );
                self.protocol.phase = Phase::Idle;
                self.protocol.followup.publish_alive = false;
            }
        }
    }

    async fn finish_settings_sync(&mut self) {
        if self.protocol.followup.publish_alive {
            self.protocol.followup.publish_alive = false;
            if let Err(err) = self.publish_alive().await {
                warn!("Failed to publish alive manifest: {err:?}");
            } else {
                info!(
                    "Completed retained settings sync pages={} rev={}",
                    self.protocol.manifest.schema_pages, self.protocol.manifest.schema_rev
                );
            }
        }
        self.protocol.phase = Phase::Idle;
        self.start_settings_sync();
    }

    fn schema_page_topic(&self, page: usize) -> String<MAX_TOPIC_LENGTH> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        topic.push_str("/schema/").ok();
        write!(&mut topic, "{page}").ok();
        topic
    }

    fn settings_sync_topic(&self, path: &str) -> String<MAX_TOPIC_LENGTH> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self.prefix.try_into().unwrap();
        topic.push_str("/settings").ok();
        topic.push_str(path).ok();
        topic
    }
}
