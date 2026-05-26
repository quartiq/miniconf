use miniconf::{Tree, leaf};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Standby,
    Run,
}

#[derive(Clone, Default, PartialEq, Eq, Tree)]
#[tree(meta(typename))]
pub struct Calibration {
    pub offset: i32,
    #[tree(meta(unit = "ppm"))]
    pub slope: i16,
}

#[derive(Clone, Default, PartialEq, Tree)]
#[tree(meta(typename))]
pub struct Settings {
    /// Hardware serial number.
    #[tree(with = read_only, meta(doc))]
    pub serial: u32,
    pub control: Control,
    pub output: Output,
    /// Factory calibration applied to measurements.
    #[tree(meta(doc))]
    pub calibration: Option<Calibration>,
    #[tree(rename = "temp", with = read_only, meta(unit = "°C"))]
    pub temperature: Option<f32>,
}

#[derive(Clone, Default, PartialEq, Eq, Tree)]
#[tree(meta(typename))]
pub struct Control {
    pub enabled: bool,
    #[tree(with = leaf)]
    pub mode: Mode,
}

#[derive(Clone, PartialEq, Eq, Tree)]
#[tree(meta(typename))]
pub struct Output {
    #[tree(with = dac, meta(max = "4095"))]
    pub dac: [u16; 2],
    #[tree(meta(unit = "dB"))]
    pub attenuation: [i16; 2],
}

impl Default for Output {
    fn default() -> Self {
        Self {
            dac: [1024, 1024],
            attenuation: [0, 0],
        }
    }
}

impl Settings {
    pub fn new() -> Self {
        Self {
            serial: 0x1234,
            control: Control {
                enabled: true,
                mode: Mode::Run,
            },
            output: Output::default(),
            calibration: Some(Calibration {
                offset: -3,
                slope: 12,
            }),
            temperature: None,
        }
    }
}

mod read_only {
    pub use miniconf::{
        deny::{deserialize_by_key, mut_any_by_key},
        passthrough::{probe_by_key, ref_any_by_key, schema, serialize_by_key},
    };
}

mod dac {
    use miniconf::{Deserializer, Keys, SerdeError, TreeDeserialize, ValueError};

    pub use miniconf::passthrough::{
        mut_any_by_key, probe_by_key, ref_any_by_key, schema, serialize_by_key,
    };

    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        value: &mut [u16; 2],
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        // Validate into a scratch copy so a rejected write leaves the DAC unchanged.
        let mut next = *value;
        next.deserialize_by_key(keys, de)?;
        if next.iter().all(|value| *value <= 4095) {
            *value = next;
            Ok(())
        } else {
            Err(ValueError::Access("DAC value exceeds 12-bit range").into())
        }
    }
}
