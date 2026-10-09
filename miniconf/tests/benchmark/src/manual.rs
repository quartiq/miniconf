use serde::{Serialize, de::DeserializeOwned};
use serde_json_core::{de::Deserializer, to_slice};

use crate::{Error, Settings};

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
