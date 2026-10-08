//! Settings commands for application-owned shells.
//!
//! Parse a line, then dispatch it alongside application commands. Use
//! `miniconf::json_core::set` for assignments and record their outcome before
//! writing a response. Persistence and hardware application belong to the caller.

use core::fmt::Debug;

use embedded_io_async::Write;
use heapless::String;
use miniconf::{ConstPath, IntoKeys, NodeIter, SerdeError, TreeSerialize, ValueError, json_core};

use crate::{MAX_DEPTH, MAX_PATH_LENGTH};

mod completion;
pub use completion::{Completion, complete, complete_path};
mod matches;
mod schema;
pub use schema::write_schema;
mod terminal;
pub use terminal::Terminal;

/// Settings command syntax.
pub const HELP: &str =
    "help, get [path], set <path> <json>, schema [path], store, reset (defaults on next boot)";

/// A borrowed settings command. JSON is retained verbatim apart from surrounding whitespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command<'a> {
    /// Show settings command syntax.
    Help,
    /// Read a leaf or subtree; an empty path selects the root.
    Get(&'a str),
    /// Describe a node and its descendants without reading settings.
    Schema(&'a str),
    /// Assign one leaf.
    Set { path: &'a str, value: &'a str },
    /// Store the current settings.
    Store,
    /// Select defaults for subsequent boots without changing current settings.
    ResetStored,
}

/// Invalid settings command syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    /// The command belongs to another handler or is unknown.
    UnknownCommand,
    /// Required arguments are missing or extra arguments were supplied.
    Arguments,
    /// A nonempty path must start with `/`.
    Path,
}

impl<'a> Command<'a> {
    /// Parse a line; empty input produces no command.
    /// Paths are unquoted, whitespace-free tokens; JSON values may contain whitespace.
    /// Empty segments are preserved: `/` selects an empty-named child, not the root.
    pub fn parse(line: &'a str) -> Result<Option<Self>, ParseError> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }
        let (name, args) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let args = args.trim();
        let command = match name {
            "help" if args.is_empty() => Self::Help,
            "get" => Self::Get(args),
            "schema" => Self::Schema(args),
            "set" => {
                let (path, value) = args
                    .split_once(char::is_whitespace)
                    .ok_or(ParseError::Arguments)?;
                let value = value.trim();
                if value.is_empty() {
                    return Err(ParseError::Arguments);
                }
                Self::Set { path, value }
            }
            "store" if args.is_empty() => Self::Store,
            "reset" if args.is_empty() => Self::ResetStored,
            "help" | "store" | "reset" => return Err(ParseError::Arguments),
            _ => return Err(ParseError::UnknownCommand),
        };
        if let Self::Get(path) | Self::Schema(path) | Self::Set { path, .. } = command {
            if path.contains(char::is_whitespace) {
                return Err(ParseError::Arguments);
            }
            path.into_keys().map_err(|_| ParseError::Path)?;
        }
        Ok(Some(command))
    }
}

/// Print current leaf values under a path, without constructing defaults.
/// Read and path errors are printed; output failures are returned.
/// The tree must fit [`MAX_DEPTH`], checked at compile time.
/// Holds `settings` until complete. For shared settings, traverse with
/// [`NodeIter`] and call [`write_value`] separately for each leaf.
pub async fn write_values<S: TreeSerialize, W: Write>(
    writer: &mut W,
    settings: &S,
    root: &str,
    scratch: &mut [u8],
) -> Result<(), W::Error> {
    const { assert!(S::SCHEMA.max_depth() <= MAX_DEPTH) }
    let nodes = match NodeIter::<ConstPath<String<MAX_PATH_LENGTH>, '/'>, MAX_DEPTH>::with_root(
        S::SCHEMA,
        root,
    ) {
        Ok(nodes) => nodes,
        Err(error) => return write_error(writer, error).await,
    };
    for path in nodes {
        let path = match path {
            Ok(path) => path,
            Err(error) => return write_error(writer, error).await,
        };
        write_value(writer, settings, path.as_ref(), scratch).await?;
    }
    Ok(())
}

/// Serialize one leaf immediately, then write its path and value without borrowing settings.
/// Read errors are printed; output failures are returned.
/// With a lock or `RefCell`, bind the future in a separate statement so the
/// settings guard drops before awaiting output.
pub fn write_value<'a, S: TreeSerialize, W: Write>(
    writer: &'a mut W,
    settings: &S,
    path: &'a str,
    scratch: &'a mut [u8],
) -> impl core::future::Future<Output = Result<(), W::Error>> + use<'a, S, W> {
    let value = json_core::get(settings, path, scratch);
    async move {
        writer.write_all(path.as_bytes()).await?;
        writer.write_all(b": ").await?;
        match value {
            Ok(len) => writer.write_all(&scratch[..len]).await?,
            Err(SerdeError::Value(ValueError::Absent)) => writer.write_all(b"<absent>").await?,
            Err(error) => return write_error(writer, error).await,
        }
        writer.write_all(b"\r\n").await
    }
}

async fn write_error<W: Write>(writer: &mut W, error: impl Debug) -> Result<(), W::Error> {
    let message = heapless::format!(256; "error: {error:?}\r\n");
    writer
        .write_all(
            message
                .as_deref()
                .unwrap_or("error (message too long)\r\n")
                .as_bytes(),
        )
        .await
}
