use minimq::{embedded_nal::IpAddr, Minimq, Property, QoS};
use serde::Serialize;
use std_embedded_nal::Stack;
use std_embedded_time::StandardClock;
use super::messages::SettingsResponse;

pub struct MiniconfClient {
    mqtt: Minimq<Stack, StandardClock, 256, 1>,
    prefix: String,
}

impl MiniconfClient {
    pub fn new(prefix: &str, broker: impl Into<IpAddr>) -> Self {
        let mut mqtt = Minimq::new(
            broker.into(),
            "",
            Stack::default(),
            StandardClock::default(),
        )
        .unwrap();

        while !mqtt.client.is_connected() {
            mqtt.poll(|_client, _topic, _payload, _properties| {})
                .unwrap();
        }

        // Subscribe to the response topic.
        let prefix: String = prefix.to_string();
        mqtt.client
            .subscribe(&(prefix.clone() + "/response"), &[])
            .unwrap();

        Self { mqtt, prefix }
    }

    pub fn configure(&mut self, path: &str, value: impl Serialize) -> Result<(), String> {
        let mut uuid_buffer = uuid::Uuid::encode_buffer();
        let identifier = uuid::Uuid::new_v4();
        let uuid_value = identifier
            .to_simple()
            .encode_upper(&mut uuid_buffer)
            .as_bytes();
        let response_topic = self.prefix.clone() + "/response";

        let properties = [
            Property::ResponseTopic(&response_topic),
            Property::CorrelationData(uuid_value),
        ];
        let data: heapless::Vec<u8, 256> = serde_json_core::to_vec(&value).unwrap();

        let topic: String = self.prefix.clone() + "/settings/" + path;
        self.mqtt
            .client
            .publish(&topic, &data, QoS::AtMostOnce, &properties)
            .unwrap();

        let mut response: Option<SettingsResponse> = None;
        loop {
            self.mqtt
                .poll(|_client, _topic, payload, properties| {
                    // Check correlation data.
                    if let Some(Property::CorrelationData(cd)) = properties
                        .iter()
                        .find(|prop| matches!(prop, Property::CorrelationData(_)))
                    {
                        if cd == &uuid_value {
                            response.replace(serde_json_core::from_slice(payload).unwrap().0);
                        }
                    }
                })
                .unwrap();

            if let Some(response) = response {
                if response.code == 0 {
                    return Ok(());
                }

                return Err(response.msg.to_string());
            }
        }
    }
}
