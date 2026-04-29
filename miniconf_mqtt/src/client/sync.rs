use core::fmt::Write as _;

use heapless::String;
use log::{debug, info};
use minimq::{ProtocolError, PubError, Publication, QoS};

use super::{Error, MqttClient, PayloadError, PublishPayload};
use crate::{
    MAX_TOPIC_LENGTH,
    message::{DepthError, simple_pub_error},
    schema::{SchemaSync, SettingsSync},
};

impl<'a, Settings, IO> MqttClient<'a, Settings, IO>
where
    Settings: miniconf::TreeSchema + miniconf::TreeSerialize + miniconf::TreeDeserializeOwned,
    IO: minimq::Io,
{
    pub(super) async fn publish_alive_once(&mut self) -> Result<(), Error<IO::Error>> {
        let mut topic: String<MAX_TOPIC_LENGTH> = self
            .prefix
            .try_into()
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        topic
            .push_str("/alive")
            .map_err(|_| Error::Mqtt(ProtocolError::BufferSize.into()))?;
        let publication = Publication::new(
            &topic,
            PublishPayload::<Settings>::Alive(&self.protocol.manifest),
        )
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
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&minimq::InboundPublish<'msg>),
    {
        self.publish_alive_once().await?;
        while !self.is_publish_quiescent() {
            self.poll_quiescent(settings, on_other).await?;
        }
        Ok(())
    }

    pub(super) async fn publish_schema<F>(
        &mut self,
        settings: &mut Settings,
        on_other: &mut F,
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&minimq::InboundPublish<'msg>),
    {
        let mut sync = SchemaSync::new(Settings::SCHEMA);
        info!("Starting schema sync defs={}", sync.defs.len());
        while sync.next != sync.defs.len() {
            while !self.is_publish_quiescent() {
                self.poll_quiescent(settings, on_other).await?;
            }
            debug!(
                "Publishing schema page={} next_def={}",
                sync.page, sync.next
            );
            let topic = self.schema_page_topic(sync.page);
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
            .qos(QoS::AtLeastOnce)
            .retain();
            match self.session.publish(publication).await {
                Ok(()) => {
                    debug!(
                        "Schema page={} published, waiting for quiescent session",
                        sync.page
                    );
                    while !self.is_publish_quiescent() {
                        self.poll_quiescent(settings, on_other).await?;
                    }
                }
                Err(PubError::Payload((true, PayloadError::Schema(id)))) => {
                    info!("Aborting schema sync after oversized schema entry for definition {id}");
                    return Err(Error::Mqtt(minimq::Error::Protocol(ProtocolError::Failed(
                        minimq::ReasonCode::PacketTooLarge,
                    ))));
                }
                Err(PubError::Payload(_)) => unreachable!(),
                Err(PubError::Session(err)) => return Err(Error::Mqtt(err)),
            }
            let Some((count, hash)) = advanced else {
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
    ) -> Result<(), Error<IO::Error>>
    where
        F: for<'msg> FnMut(&minimq::InboundPublish<'msg>),
    {
        let mut iter = SettingsSync::new(Settings::SCHEMA);
        info!("Starting retained settings sync");
        while let Some(path) = iter.next() {
            while !self.is_publish_quiescent() {
                self.poll_quiescent(settings, on_other).await?;
            }
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
            match self.try_publish_leaf(settings, &state[..depth]).await {
                Ok(()) => {
                    debug!("Published retained setting {}", path);
                    while !self.is_publish_quiescent() {
                        self.poll_quiescent(settings, on_other).await?;
                    }
                }
                Err(PubError::Payload((
                    _no_space,
                    PayloadError::Leaf(DepthError {
                        inner:
                            miniconf::SerdeError::Value(
                                miniconf::ValueError::Absent | miniconf::ValueError::Access(_),
                            ),
                        ..
                    }),
                ))) => {
                    self.clear_leaf(&topic).await?;
                    while !self.is_publish_quiescent() {
                        self.poll_quiescent(settings, on_other).await?;
                    }
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
