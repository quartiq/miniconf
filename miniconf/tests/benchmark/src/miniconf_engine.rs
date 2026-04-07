use crate::Command;
use crate::codec::{Response, ResponseSerializer, ValueDeserializer};
use crate::settings::Settings;
use miniconf::{IntoKeys, Path, TreeDeserialize, TreeSerialize};

pub struct Engine {
    settings: Settings,
}

pub enum MiniconfError {
    Serde,
}

impl Engine {
    fn serialize(&self, path: &str, out: &mut Response) -> Result<(), MiniconfError> {
        self.settings
            .serialize_by_key(
                Path::<_, '/'>(path).into_keys(),
                ResponseSerializer::new(out),
            )
            .map_err(|_| MiniconfError::Serde)
    }
}

impl crate::Engine for Engine {
    const NAME: &'static str = "miniconf_path";
    type Error = MiniconfError;

    fn new() -> Self {
        Self {
            settings: Settings::new(),
        }
    }

    fn exec(&mut self, cmd: Command<'_>, out: &mut Response) -> Result<(), Self::Error> {
        match cmd {
            Command::Get(path) => self.serialize(path, out),
            Command::Set(path, input) => {
                self.settings
                    .deserialize_by_key(
                        Path::<_, '/'>(path).into_keys(),
                        ValueDeserializer::new(input),
                    )
                    .map_err(|_| MiniconfError::Serde)?;
                self.serialize(path, out)
            }
        }
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}
