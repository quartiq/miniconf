use serde::{Serialize, de::DeserializeOwned};
use serde_json_core::{de::Deserializer, to_slice};

use crate::{Error, Settings};

#[cfg(feature = "help")]
pub(super) fn help(path: &str, mut reply: impl FnMut(&[u8])) -> Result<(), Error> {
    let text = match path {
        "" => {
            "/ typename=Settings\n  serial doc=Hardware serial number. type=U32\n  control typename=Control\n  output typename=Output\n  calibration typename=Calibration doc=Factory calibration applied to measurements. optional\n  temp unit=°C type=F32 optional"
        }
        "/serial" => "/serial doc=Hardware serial number. type=U32",
        "/control" => "/control typename=Control\n  enabled type=Bool\n  mode",
        "/control/enabled" => "/control/enabled type=Bool",
        "/control/mode" => "/control/mode",
        "/output" => "/output typename=Output\n  dac max=4095\n  attenuation unit=dB",
        "/output/dac" => "/output/dac max=4095\n  0 type=U16\n  1 type=U16",
        "/output/dac/0" => "/output/dac/0 type=U16",
        "/output/dac/1" => "/output/dac/1 type=U16",
        "/output/attenuation" => "/output/attenuation unit=dB\n  0 type=I16\n  1 type=I16",
        "/output/attenuation/0" => "/output/attenuation/0 type=I16",
        "/output/attenuation/1" => "/output/attenuation/1 type=I16",
        "/calibration" => {
            "/calibration typename=Calibration doc=Factory calibration applied to measurements. optional\n  offset type=I32\n  slope unit=ppm type=I16"
        }
        "/calibration/offset" => "/calibration/offset type=I32",
        "/calibration/slope" => "/calibration/slope unit=ppm type=I16",
        "/temp" => "/temp unit=°C type=F32 optional",
        _ => return Err(Error::Path),
    };
    for line in text.lines() {
        reply(line.as_bytes());
    }
    Ok(())
}

fn value<T: Serialize + DeserializeOwned>(
    value: &mut T,
    input: Option<&str>,
    out: &mut [u8],
) -> Result<usize, Error> {
    if let Some(input) = input {
        let mut de = Deserializer::new(input.as_bytes(), None);
        let next = T::deserialize(&mut de).map_err(|_| Error::Value)?;
        de.end().map_err(|_| Error::Value)?;
        *value = next;
        Ok(0)
    } else {
        to_slice(value, out).map_err(|_| Error::Value)
    }
}

pub(super) fn exchange(
    settings: &mut Settings,
    path: &str,
    input: Option<&str>,
    out: &mut [u8],
) -> Result<usize, Error> {
    let len = match path {
        "/serial" | "/temp" if input.is_some() => return Err(Error::Access),
        "/serial" => to_slice(&settings.serial, out).map_err(|_| Error::Value)?,
        "/temp" => to_slice(
            settings.temperature.as_ref().ok_or(Error::Unavailable)?,
            out,
        )
        .map_err(|_| Error::Value)?,
        "/control/enabled" => value(&mut settings.control.enabled, input, out)?,
        "/control/mode" => value(&mut settings.control.mode, input, out)?,
        "/calibration/offset" => value(
            &mut settings
                .calibration
                .as_mut()
                .ok_or(Error::Unavailable)?
                .offset,
            input,
            out,
        )?,
        "/calibration/slope" => value(
            &mut settings
                .calibration
                .as_mut()
                .ok_or(Error::Unavailable)?
                .slope,
            input,
            out,
        )?,
        _ => {
            let (array, index) = path
                .strip_prefix("/output/")
                .ok_or(Error::Path)?
                .split_once('/')
                .ok_or(Error::Path)?;
            let index: usize = index.parse().map_err(|_| Error::Path)?;
            match array {
                "dac" => {
                    let mut next = settings.output.dac;
                    let len = value(next.get_mut(index).ok_or(Error::Path)?, input, out)?;
                    if input.is_some() {
                        if next.iter().any(|v| *v > 4095) {
                            return Err(Error::Access);
                        }
                        settings.output.dac = next;
                    }
                    len
                }
                "attenuation" => value(
                    settings
                        .output
                        .attenuation
                        .get_mut(index)
                        .ok_or(Error::Path)?,
                    input,
                    out,
                )?,
                _ => return Err(Error::Path),
            }
        }
    };
    Ok(len)
}
