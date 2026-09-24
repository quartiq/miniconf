use crate::codec::{Response, ResponseSerializer, ValueDeserializer};
use crate::settings::Settings;
#[cfg(feature = "erased-keys")]
use miniconf::Keys;
use miniconf::{IntoKeys, TreeDeserialize, TreeSerialize};

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
        let keys = path.into_keys();
        #[cfg(feature = "erased-keys")]
        let keys = &mut { keys } as &mut dyn Keys;
        self.settings
            .deserialize_by_key(keys, ValueDeserializer::new(value))
            .map_err(|_| MiniconfError::Serde)
    }

    fn get(&self, path: &str, out: &mut Response) -> Result<(), Self::Error> {
        let keys = path.into_keys();
        #[cfg(feature = "erased-keys")]
        let keys = &mut { keys } as &mut dyn Keys;
        self.settings
            .serialize_by_key(keys, ResponseSerializer::new(out))
            .map_err(|_| MiniconfError::Serde)
    }
}
