use core::cell::Cell;
use core::fmt::Write as _;

use heapless::String;
use log::{debug, info};
use minimq::{ProtocolError, PubError, Publication, QoS};
use serde::Serialize;

use super::{Error, MqttClient};
use crate::{
    EncodeError, MAX_TOPIC_LENGTH,
    message::{DepthError, simple_pub_error},
    schema::{SchemaSync, SettingsSync, serialize_schema_page},
};

impl<'a, Settings, C> MqttClient<'a, Settings, C>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
    C: minimq::transport::Connector,
{
    pub(super) async fn publish_alive_once(&mut self) -> Result<(), Error<C::Error>> {
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

    pub(super) async fn publish_alive<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<C::Error>>
    where
        F: for<'msg> FnMut(&minimq::InboundPublish<'msg>),
    {
        self.publish_alive_once().await?;
        self.wait_publish_quiescent(settings, on_other).await
    }

    pub(super) async fn publish_schema<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<C::Error>>
    where
        F: for<'msg> FnMut(&minimq::InboundPublish<'msg>),
    {
        let mut sync = SchemaSync::new(Settings::SCHEMA);
        info!("Starting schema sync defs={}", sync.defs.len());
        while sync.next != sync.defs.len() {
            debug_assert!(self.session.is_publish_quiescent());
            debug!(
                "Publishing schema page={} next_def={}",
                sync.page, sync.next
            );
            let topic = self.schema_page_topic(sync.page);
            let advanced = Cell::new(None::<(usize, u32)>);
            let publication = Publication::new(&topic, |buf: &mut [u8]| {
                let page =
                    serialize_schema_page(&sync.defs, sync.next, buf).map_err(|id| (true, id))?;
                let next_hash = yafnv::Fnv::fnv1a(sync.hash, buf[..page.len].iter().copied());
                advanced.set(Some((page.count, next_hash)));
                Ok::<usize, EncodeError<usize>>(page.len)
            })
            .qos(QoS::AtLeastOnce)
            .retain();
            match self.session.publish(publication).await {
                Ok(()) => {
                    debug!(
                        "Schema page={} published, waiting for quiescent session",
                        sync.page
                    );
                    self.wait_publish_quiescent(settings, on_other).await?
                }
                Err(PubError::Payload((true, id))) => {
                    info!("Aborting schema sync after oversized schema entry for definition {id}");
                    return Err(Error::Mqtt(minimq::Error::Protocol(ProtocolError::Failed(
                        minimq::ReasonCode::PacketTooLarge,
                    ))));
                }
                Err(PubError::Payload((false, _))) => unreachable!(),
                Err(PubError::Session(err)) => return Err(Error::Mqtt(err)),
            }
            let Some((count, hash)) = advanced.get() else {
                return Err(Error::Mqtt(ProtocolError::BufferSize.into()));
            };
            sync.next += count;
            sync.page += 1;
            sync.hash = hash;
        }
        self.protocol.manifest.schema_pages = sync.page;
        self.protocol.manifest.schema_rev = sync.hash;
        info!(
            "Completed schema sync pages={} rev={}",
            self.protocol.manifest.schema_pages, self.protocol.manifest.schema_rev
        );
        Ok(())
    }

    pub(super) async fn publish_settings<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<C::Error>>
    where
        F: for<'msg> FnMut(&minimq::InboundPublish<'msg>),
    {
        let mut iter = SettingsSync::new(Settings::SCHEMA);
        info!("Starting retained settings sync");
        while let Some(path) = iter.next() {
            debug_assert!(self.session.is_publish_quiescent());
            let path = path
                .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?
                .into_inner();
            let full = iter
                .indices()
                .ok_or_else(|| Error::Mqtt(ProtocolError::BufferSize.into()))?;
            let mut state = [0; crate::MAX_DEPTH];
            state[..full.len()].copy_from_slice(full);
            let depth = full.len();
            let topic = self.settings_sync_topic(&path);
            match self.try_publish_leaf(settings, state, depth).await {
                Ok(()) => {
                    debug!("Published retained setting {}", path);
                    self.wait_publish_quiescent(settings, on_other).await?
                }
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
                    self.clear_leaf(&topic).await?;
                    self.wait_publish_quiescent(settings, on_other).await?;
                }
                Err(err) => return Err(simple_pub_error(err)),
            }
        }
        info!(
            "Completed retained settings sync pages={} rev={}",
            self.protocol.manifest.schema_pages, self.protocol.manifest.schema_rev
        );
        Ok(())
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
