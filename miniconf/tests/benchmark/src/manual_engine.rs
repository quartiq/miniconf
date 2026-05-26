use crate::codec::{CodecError, Response, parse_bool, parse_i16, parse_i32, parse_u16};
use crate::settings::Settings;

pub struct Engine {
    settings: Settings,
}

pub enum ManualError {
    InvalidPath,
    Absent,
    Codec(CodecError),
}

macro_rules! manual_leafs {
    ($m:ident) => {
        $m! {
            Serial, "/serial",
            |s: &Engine, o: &mut Response| o.write_u32(s.settings.serial).map_err(ManualError::Codec),
            |_s: &mut Engine, _i: &str| Err(ManualError::InvalidPath);

            ControlEnabled, "/control/enabled",
            |s: &Engine, o: &mut Response| o.write_bool(s.settings.control.enabled).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.control.enabled = parse_bool(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            OutputDac0, "/output/dac/0",
            |s: &Engine, o: &mut Response| o.write_u16(s.settings.output.dac[0]).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                let value = parse_u16(i).map_err(ManualError::Codec)?;
                if value > 4095 {
                    return Err(ManualError::InvalidPath);
                }
                s.settings.output.dac[0] = value;
                Ok(())
            };

            OutputDac1, "/output/dac/1",
            |s: &Engine, o: &mut Response| o.write_u16(s.settings.output.dac[1]).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                let value = parse_u16(i).map_err(ManualError::Codec)?;
                if value > 4095 {
                    return Err(ManualError::InvalidPath);
                }
                s.settings.output.dac[1] = value;
                Ok(())
            };

            OutputAttenuation0, "/output/attenuation/0",
            |s: &Engine, o: &mut Response| o.write_i16(s.settings.output.attenuation[0]).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.output.attenuation[0] = parse_i16(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            OutputAttenuation1, "/output/attenuation/1",
            |s: &Engine, o: &mut Response| o.write_i16(s.settings.output.attenuation[1]).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.output.attenuation[1] = parse_i16(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            CalibrationOffset, "/calibration/offset",
            |s: &Engine, o: &mut Response| {
                o.write_i32(s.settings.calibration.as_ref().ok_or(ManualError::Absent)?.offset)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.calibration.as_mut().ok_or(ManualError::Absent)?.offset =
                    parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            CalibrationSlope, "/calibration/slope",
            |s: &Engine, o: &mut Response| {
                o.write_i16(s.settings.calibration.as_ref().ok_or(ManualError::Absent)?.slope)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.calibration.as_mut().ok_or(ManualError::Absent)?.slope =
                    parse_i16(i).map_err(ManualError::Codec)?;
                Ok(())
            };
        }
    };
}

macro_rules! define_key {
    ( $( $key:ident, $path:literal, $get:expr, $set:expr; )* ) => {
        #[derive(Copy, Clone)]
        enum Key {
            $( $key, )*
        }

        impl Key {
            fn parse(path: &str) -> Result<Self, ManualError> {
                match path {
                    $( $path => Ok(Self::$key), )*
                    _ => Err(ManualError::InvalidPath),
                }
            }
        }
    };
}

manual_leafs!(define_key);

macro_rules! impl_leaf_access {
    ( $( $key:ident, $path:literal, $get:expr, $set:expr; )* ) => {
        fn serialize_key(&self, key: Key, out: &mut Response) -> Result<(), ManualError> {
            out.clear();
            match key {
                $( Key::$key => ($get)(self, out), )*
            }
        }

        fn deserialize_key(&mut self, key: Key, input: &str) -> Result<(), ManualError> {
            match key {
                $( Key::$key => ($set)(self, input), )*
            }
        }
    };
}

impl Engine {
    manual_leafs!(impl_leaf_access);
}

impl crate::Engine for Engine {
    type Error = ManualError;

    fn new() -> Self {
        Self {
            settings: Settings::new(),
        }
    }

    fn set(&mut self, path: &str, value: &str) -> Result<(), Self::Error> {
        self.deserialize_key(Key::parse(path)?, value)
    }

    fn get(&self, path: &str, out: &mut Response) -> Result<(), Self::Error> {
        self.serialize_key(Key::parse(path)?, out)
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}
