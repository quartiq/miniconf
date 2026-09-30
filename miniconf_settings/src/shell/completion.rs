use core::{fmt::Write, ops::Range};

use heapless::String;
use miniconf::{IntoKeys, Schema};

use crate::MAX_PATH_LENGTH;

/// A command-line edit or the help to show when completion cannot advance.
/// Byte ranges refer to the unchanged input line.
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    /// Replace a byte range and place the cursor after the replacement.
    Replace(Range<usize>, String<MAX_PATH_LENGTH>),
    /// Show command help (`None`) or schema for a path's byte range (`Some`).
    Help(Option<Range<usize>>),
}

/// Complete command names or paths for `get`, `set`, and `schema`.
///
/// `commands` lists the application's available commands, including its own.
/// Ambiguous matches extend their common prefix. Values are not completed.
/// A unique leaf at the end of the path is followed by a space.
/// Names containing whitespace are omitted from command completion.
/// Arguments of application commands return `None`; use [`complete_path`] to
/// compose path completion with a different command syntax.
/// The caller decides when to display help (for example, on repeated Tab).
pub fn complete(
    schema: &'static Schema,
    line: &str,
    cursor: usize,
    commands: &[&str],
) -> Option<Completion> {
    let mut result: Option<(Range<usize>, String<MAX_PATH_LENGTH>)> = None;
    let mut unique = true;
    let mut candidate = |range: Range<usize>, text: &str| match &mut result {
        None => result = String::try_from(text).ok().map(|text| (range, text)),
        Some((_, prefix)) => {
            unique = false;
            let len = prefix
                .chars()
                .zip(text.chars())
                .take_while(|(a, b)| a == b)
                .map(|(c, _)| c.len_utf8())
                .sum();
            prefix.truncate(len);
        }
    };
    let before = line.get(..cursor)?;
    let start = line.len() - line.trim_start().len();
    let end = start + line[start..].split_whitespace().next().unwrap_or("").len();
    if cursor < start {
        return None;
    }
    let mut help = None;
    if cursor <= end {
        for name in commands {
            if !name.contains(char::is_whitespace) && name.starts_with(&before[start..]) {
                let mut text = String::<MAX_PATH_LENGTH>::try_from(*name).ok()?;
                if end == line.len() {
                    text.push(' ').ok()?;
                }
                candidate(start..end, &text);
            }
        }
    } else if commands.contains(&&line[start..end])
        && matches!(&line[start..end], "get" | "set" | "schema")
    {
        let args = end + line[end..].len() - line[end..].trim_start().len();
        if cursor < args || line[args..cursor].contains(char::is_whitespace) {
            return None;
        }
        let path_end = args
            + line[args..]
                .find(char::is_whitespace)
                .unwrap_or(line.len() - args);
        help = Some(args..cursor);
        complete_path(
            schema,
            &line[args..path_end],
            cursor - args,
            |range, text| {
                if !text.contains(char::is_whitespace) {
                    candidate(args + range.start..args + range.end, text);
                }
            },
        );
    } else {
        return None;
    }
    if let Some((mut range, mut text)) = result {
        if unique
            && let Some(root) = &help
            && !line[range.end..].starts_with('/')
            && let Some(parent) = line[root.start..range.start].strip_suffix('/')
            && schema
                .get(parent.chain([text.as_str()]))
                .is_ok_and(|node| node.schema.is_leaf())
        {
            text.push(' ').ok()?;
            if let Some(space) = line[range.end..].chars().next() {
                range.end += space.len_utf8();
            }
        }
        if !text.is_empty() && (line[range.clone()] != text || cursor != range.start + text.len()) {
            return Some(Completion::Replace(range, text));
        }
    }
    if let Some(range) = &mut help
        && schema.get(&line[range.clone()]).is_err()
    {
        let parent = line[range.clone()].rsplit_once('/')?.0;
        schema.get(parent).ok()?;
        range.end = range.start + parent.len();
    }
    Some(Completion::Help(help))
}

/// Offer replacements for the path component at a UTF-8 byte cursor.
///
/// Each callback receives a byte range in `path` and replacement text. Later
/// components remain untouched; no trailing separator is added. Candidates describe
/// schema possibilities, not runtime presence or writability. Invalid parent paths or
/// cursor positions yield none. Components exceeding `MAX_PATH_LENGTH` bytes
/// are omitted.
/// Replacement text is borrowed only for the callback; no allocator is needed.
pub fn complete_path(
    schema: &'static Schema,
    path: &str,
    cursor: usize,
    mut candidate: impl FnMut(Range<usize>, &str),
) {
    let Some(before) = path.get(..cursor) else {
        return;
    };
    if path.is_empty() && !schema.is_leaf() {
        candidate(0..0, "/");
        return;
    }
    let Some((parent, prefix)) = before.rsplit_once('/') else {
        return;
    };
    let Ok(node) = schema.get(parent) else {
        return;
    };
    let Some(children) = node.schema.internal() else {
        return;
    };
    let start = before.len() - prefix.len();
    let end = cursor + path[cursor..].find('/').unwrap_or(path.len() - cursor);
    for index in 0..children.len().get() {
        let mut text = String::<MAX_PATH_LENGTH>::new();
        let name = match children.get_name(index) {
            Some(name) => text.write_str(name),
            None => write!(text, "{index}"),
        };
        if name.is_err() || !text.starts_with(prefix) {
            continue;
        }
        candidate(start..end, &text);
    }
}
