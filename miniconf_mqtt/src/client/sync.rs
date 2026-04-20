use core::cell::Cell;
use core::fmt::Write as _;

#[cfg(feature = "compat-settings-ingress")]
use embassy_time::Instant;
use heapless::String;
use log::{debug, info, warn};
use minimq::{ProtocolError, PubError, Publication, QoS};
use serde::Serialize;

#[cfg(feature = "compat-settings-ingress")]
use super::SettingsIngressPhase;
use super::{Error, MqttClient};
use crate::{
    MAX_TOPIC_LENGTH, json_slice,
    message::{DepthError, simple_pub_error},
    schema::{Pending, SchemaPageError, serialize_schema_page},
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
            json_slice(&body, buf).map(|text| text.len())
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
            } if matches!(self.protocol.pending, Pending::Idle) => Some(deadline),
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
        if !idle || Instant::now() < deadline || !matches!(self.protocol.pending, Pending::Idle) {
            return;
        }
        self.protocol.settings_ingress = SettingsIngressPhase::Runtime;
        debug!("Finished settings ingress recovery");
        self.protocol.pending = Pending::settings(Settings::SCHEMA);
    }

    #[cfg(not(feature = "compat-settings-ingress"))]
    pub(super) fn finish_settings_recovery(&mut self, _idle: bool) {}

    pub(super) async fn advance_pending(&mut self, settings: &Settings) {
        if !self.session.can_publish(QoS::AtLeastOnce) {
            return;
        }
        match &mut self.protocol.pending {
            Pending::Idle => {}
            Pending::Schema { .. } => self.advance_schema_pending().await,
            Pending::Settings { .. } => self.advance_settings_pending(settings).await,
        }
    }

    async fn advance_schema_pending(&mut self) {
        let (root, defs, next, page, hash) = match self.protocol.pending {
            Pending::Schema {
                root,
                defs,
                next,
                page,
                hash,
            } => (root, defs, next, page, hash),
            _ => unreachable!(),
        };
        if next == defs {
            self.finish_schema_sync(page, hash);
            return;
        }

        let topic = self.schema_page_topic(page);
        let advanced = Cell::new(None::<(usize, u32)>);
        let publication = Publication::new(&topic, |buf: &mut [u8]| {
            let page = serialize_schema_page(root, next, buf)?;
            let next_hash = yafnv::Fnv::fnv1a(hash, buf[..page.len].iter().copied());
            advanced.set(Some((page.count, next_hash)));
            Ok(page.len)
        })
        .qos(QoS::AtLeastOnce)
        .retain();
        if let Err(err) = self.session.publish(publication).await {
            match err {
                PubError::Payload(SchemaPageError::Oversized { id }) => {
                    warn!("Aborting schema sync after oversized schema entry for definition {id}");
                }
                err => {
                    warn!(
                        "Failed to publish schema page {}: {:?}",
                        page,
                        simple_pub_error(err)
                    );
                }
            }
            self.protocol.pending.clear();
            return;
        }

        let Some((count, hash)) = advanced.get() else {
            self.protocol.pending.clear();
            return;
        };
        let Pending::Schema {
            next,
            page,
            hash: current_hash,
            ..
        } = &mut self.protocol.pending
        else {
            unreachable!()
        };
        *next += count;
        *page += 1;
        *current_hash = hash;
        let finished = *next == defs;
        let pages = *page;
        let hash = *current_hash;
        if finished {
            self.finish_schema_sync(pages, hash);
        }
    }

    fn finish_schema_sync(&mut self, pages: usize, hash: u32) {
        self.protocol.manifest.schema_pages = pages;
        self.protocol.manifest.schema_rev = hash;
        self.protocol.publish_alive_after_sync = true;
        info!(
            "Completed schema sync pages={} rev={}",
            self.protocol.manifest.schema_pages, self.protocol.manifest.schema_rev
        );
        #[cfg(feature = "compat-settings-ingress")]
        if matches!(
            self.protocol.settings_ingress,
            SettingsIngressPhase::Recovering { seen: true, .. }
        ) {
            debug!("Deferring retained settings sync until recovery completes");
            self.protocol.pending.clear();
            return;
        }
        debug!("Queued retained settings sync after schema sync");
        self.protocol.pending = Pending::settings(Settings::SCHEMA);
    }

    async fn advance_settings_pending(&mut self, settings: &Settings) {
        let (path, state, depth) = {
            let Pending::Settings { iter } = &mut self.protocol.pending else {
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
                    self.protocol.publish_alive_after_sync = false;
                    self.protocol.pending.clear();
                    return;
                }
            };
            let Some(full) = iter.state() else {
                self.protocol.pending.clear();
                return;
            };
            let mut state = [0; crate::MAX_DEPTH];
            state[..full.len()].copy_from_slice(full);
            (path, state, full.len())
        };

        let topic = self.settings_sync_topic(&path);
        match self.try_publish_leaf(settings, state, depth).await {
            Ok(()) => {}
            Err(PubError::Payload(DepthError {
                inner:
                    miniconf::SerdeError::Value(
                        miniconf::ValueError::Absent | miniconf::ValueError::Access(_),
                    ),
                ..
            })) => {
                if let Err(err) = self.clear_leaf(&topic).await {
                    warn!("Failed to clear retained setting path={path}: {err:?}");
                    self.protocol.publish_alive_after_sync = false;
                    self.protocol.pending.clear();
                }
            }
            Err(err) => {
                warn!(
                    "Failed to publish retained setting path={path}: {:?}",
                    simple_pub_error(err)
                );
                self.protocol.publish_alive_after_sync = false;
                self.protocol.pending.clear();
            }
        }
    }

    async fn finish_settings_sync(&mut self) {
        if self.protocol.publish_alive_after_sync {
            self.protocol.publish_alive_after_sync = false;
            if let Err(err) = self.publish_alive().await {
                warn!("Failed to publish alive manifest: {err:?}");
            } else {
                info!(
                    "Completed retained settings sync pages={} rev={}",
                    self.protocol.manifest.schema_pages, self.protocol.manifest.schema_rev
                );
            }
        }
        self.protocol.pending.clear();
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
