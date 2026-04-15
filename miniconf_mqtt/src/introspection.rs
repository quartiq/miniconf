use heapless::String;
use miniconf::{Schema, SerdeError, TreeSerialize, ValueError, json_core, meta_contains};
use serde::Serialize;

use crate::MAX_RESPONSE_LENGTH;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Presence {
    Present,
    Absent,
    Unknown,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuntimeState {
    Present,
    Absent,
    Unknown,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct StateInfo<'a> {
    pub(crate) state: RuntimeState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) active: Option<&'a str>,
}

fn probe_presence<Settings>(settings: &Settings, keys: impl miniconf::Keys) -> Presence
where
    Settings: TreeSerialize,
{
    match json_core::get_by_keys(settings, keys, &mut []) {
        Ok(_) | Err(SerdeError::Value(ValueError::Key(_))) => Presence::Present,
        Err(SerdeError::Value(ValueError::Absent)) => Presence::Absent,
        Err(SerdeError::Value(ValueError::Access(_)))
        | Err(SerdeError::Inner(_) | SerdeError::Finalization(_)) => Presence::Unknown,
    }
}

fn probe_path<const Y: usize>(prefix: &[usize], extra: &[usize]) -> [usize; Y] {
    debug_assert!(prefix.len() + extra.len() <= Y);
    let mut probe = [0; Y];
    probe[..prefix.len()].copy_from_slice(prefix);
    probe[prefix.len()..prefix.len() + extra.len()].copy_from_slice(extra);
    probe
}

fn child_presence<Settings, const Y: usize>(
    settings: &Settings,
    prefix: &[usize],
    index: usize,
    schema: &'static Schema,
) -> Presence
where
    Settings: TreeSerialize,
{
    let extra = if schema.is_leaf() { 0 } else { schema.len() };
    let probe = probe_path::<Y>(prefix, &[index, extra]);
    probe_presence(settings, &mut &probe[..prefix.len() + 2])
}

pub(crate) fn state_info<Settings, const Y: usize>(
    settings: &Settings,
    prefix: &[usize],
    schema: &'static Schema,
) -> StateInfo<'static>
where
    Settings: TreeSerialize,
{
    let extra = if schema.is_leaf() { 0 } else { schema.len() };
    let probe = probe_path::<Y>(prefix, &[extra]);
    match probe_presence(settings, &mut &probe[..prefix.len() + 1]) {
        Presence::Absent => StateInfo {
            state: RuntimeState::Absent,
            active: None,
        },
        Presence::Unknown => StateInfo {
            state: RuntimeState::Unknown,
            active: None,
        },
        Presence::Present => {
            let active = match (schema.internal.as_ref(), schema.meta.as_ref()) {
                (Some(miniconf::Internal::Named(children)), Some(meta))
                    if meta_contains(meta, "enum", "oneof") =>
                {
                    let mut active = None;
                    for (index, child) in children.iter().enumerate() {
                        match child_presence::<_, Y>(settings, prefix, index, child.schema) {
                            Presence::Present => {
                                if active.is_some() {
                                    return StateInfo {
                                        state: RuntimeState::Unknown,
                                        active: None,
                                    };
                                }
                                active = Some(child.name);
                            }
                            Presence::Absent => {}
                            Presence::Unknown => {
                                return StateInfo {
                                    state: RuntimeState::Unknown,
                                    active: None,
                                };
                            }
                        }
                    }
                    active
                }
                _ => None,
            };
            StateInfo {
                state: RuntimeState::Present,
                active,
            }
        }
    }
}

pub(crate) fn json_text<T: Serialize>(value: &T) -> Result<String<MAX_RESPONSE_LENGTH>, ()> {
    let mut buf = [0u8; MAX_RESPONSE_LENGTH];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    value.serialize(&mut ser).map_err(|_| ())?;
    let len = ser.end();
    let text = core::str::from_utf8(&buf[..len]).map_err(|_| ())?;
    let mut out = String::new();
    out.push_str(text).map_err(|_| ())?;
    Ok(out)
}
