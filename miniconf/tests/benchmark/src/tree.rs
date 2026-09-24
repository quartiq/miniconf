use miniconf::{SerdeError, ValueError, json_core};
#[cfg(feature = "help")]
use {
    core::fmt::Write,
    heapless::String,
    miniconf::{Meta, Schema, TreeSchema},
};

use crate::{Error, Settings};

#[cfg(feature = "help")]
fn describe(schema: &Schema, edge: &Meta, line: &mut String<128>) -> core::fmt::Result {
    for (key, value) in schema.node_meta().iter().chain(edge.iter()) {
        line.write_str(" ")?;
        line.write_str(key)?;
        line.write_str("=")?;
        line.write_str(value)?;
    }
    if let Some(sem) = schema.sem() {
        if let Some(ty) = sem.ty() {
            write!(line, " type={ty:?}")?;
        }
        if sem.maybe_absent() {
            line.write_str(" optional")?;
        }
    }
    Ok(())
}

#[cfg(feature = "help")]
pub(super) fn help(path: &str, mut reply: impl FnMut(&[u8])) -> Result<(), Error> {
    let (schema, edge) = if let Some((parent, name)) = path.rsplit_once('/') {
        let parent = Settings::SCHEMA
            .get(parent)
            .map_err(|_| Error::Path)?
            .schema;
        let internal = parent.internal().ok_or(Error::Path)?;
        let index = internal.get_index(name).ok_or(Error::Path)?;
        (internal.get_schema(index), internal.get_edge_meta(index))
    } else {
        (Settings::SCHEMA, &Meta::EMPTY)
    };
    let mut line = String::<128>::new();
    line.push_str(if path.is_empty() { "/" } else { path })
        .map_err(|_| Error::Value)?;
    describe(schema, edge, &mut line).map_err(|_| Error::Value)?;
    reply(line.as_bytes());
    if let Some(internal) = schema.internal() {
        for index in 0..internal.len().get() {
            line.clear();
            match internal.get_name(index) {
                Some(name) => line.write_str("  ").and_then(|()| line.write_str(name)),
                None => write!(line, "  {index}"),
            }
            .map_err(|_| Error::Value)?;
            describe(
                internal.get_schema(index),
                internal.get_edge_meta(index),
                &mut line,
            )
            .map_err(|_| Error::Value)?;
            reply(line.as_bytes());
        }
    }
    Ok(())
}

impl<E> From<SerdeError<E>> for Error {
    fn from(error: SerdeError<E>) -> Self {
        match error {
            SerdeError::Value(ValueError::Key(_)) => Self::Path,
            SerdeError::Value(ValueError::Absent) => Self::Unavailable,
            SerdeError::Value(ValueError::Access(_)) => Self::Access,
            SerdeError::Inner(_) | SerdeError::Finalization(_) => Self::Value,
        }
    }
}

pub(super) fn exchange(
    settings: &mut Settings,
    path: &str,
    input: Option<&str>,
    out: &mut [u8],
) -> Result<usize, Error> {
    if let Some(input) = input {
        json_core::set(settings, path, input.as_bytes()).map_err(Error::from)
    } else {
        json_core::get(settings, path, out).map_err(Error::from)
    }
}
