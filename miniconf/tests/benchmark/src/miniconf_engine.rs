use crate::codec::{Response, ResponseSerializer, ValueDeserializer};
use crate::settings::Settings;
use miniconf::{ConstPath, IntoKeys, TreeDeserialize, TreeSerialize};

pub struct Engine {
    settings: Settings,
}

pub enum MiniconfError {
    Serde,
}

impl crate::Engine for Engine {
    type Error = MiniconfError;

    fn new() -> Self {
        Self {
            settings: Settings::new(),
        }
    }

    fn set(&mut self, path: &str, value: &str) -> Result<(), Self::Error> {
        self.settings
            .deserialize_by_key(ConstPath::<_, '/'>(path).into_keys(), ValueDeserializer::new(value))
            .map_err(|_| MiniconfError::Serde)
    }

    fn get(&self, path: &str, out: &mut Response) -> Result<(), Self::Error> {
        self.settings
            .serialize_by_key(ConstPath::<_, '/'>(path).into_keys(), ResponseSerializer::new(out))
            .map_err(|_| MiniconfError::Serde)
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}
