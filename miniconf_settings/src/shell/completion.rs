use core::ops::Range;

use heapless::String;
use miniconf::Schema;

use super::matches::Candidates;
use crate::MAX_PATH_LENGTH;

/// A command-line edit or matches to show when completion cannot advance.
/// Byte ranges refer to the unchanged input line.
#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    /// Replace a byte range and place the cursor after the replacement.
    Replace(Range<usize>, String<MAX_PATH_LENGTH>),
    /// List matching commands (`None`) or paths (`Some`, ending at the cursor).
    Matches(Option<Range<usize>>),
}

/// Complete command names or paths for `get`, `set`, and `schema`.
///
/// `commands` lists the application's available commands, including its own.
/// Ambiguous matches extend their common prefix. Values are not completed.
/// A unique leaf at the end of the path is followed by a space.
/// A valid numeric index is accepted directly, even if longer indices share its prefix.
/// Names containing whitespace or control characters are omitted from command completion.
/// Arguments of application commands return `None`; use [`complete_path`] to
/// compose path completion with a different command syntax.
/// The caller decides when to list matches (for example, on repeated Tab).
pub fn complete(
    schema: &'static Schema,
    line: &str,
    cursor: usize,
    commands: &[&str],
) -> Option<Completion> {
    let mut result: Option<(Range<usize>, String<MAX_PATH_LENGTH>, bool)> = None;
    let mut unique = true;
    let mut candidate = |range: Range<usize>, text: &str, leaf: bool| match &mut result {
        None => result = String::try_from(text).ok().map(|text| (range, text, leaf)),
        Some((_, prefix, _)) => {
            unique &= prefix.as_str() == text;
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
    let mut path_range = None;
    if cursor <= end {
        for name in commands {
            if !name.chars().any(|ch| ch.is_whitespace() || ch.is_control())
                && name.starts_with(&before[start..])
            {
                candidate(start..end, name, false);
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
        path_range = Some(args..cursor);
        let path = &line[args..path_end];
        if path.is_empty() && !schema.is_leaf() {
            candidate(args..args, "/", false);
        } else {
            let (range, matches) = Candidates::path(schema, path, cursor - args)?;
            let limit = if matches.numeric_len().is_some() {
                2
            } else {
                usize::MAX
            };
            for (text, branch) in matches.iter().take(limit) {
                if !text.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
                    candidate(args + range.start..args + range.end, &text, !branch);
                }
            }
        }
    } else {
        return None;
    }
    if let Some((mut range, mut text, leaf)) = result {
        if unique && path_range.is_none() && end == line.len() {
            text.push(' ').ok()?;
        }
        if unique && path_range.is_some() && leaf && !line[range.end..].starts_with('/') {
            text.push(' ').ok()?;
            if let Some(space) = line[range.end..].chars().next() {
                range.end += space.len_utf8();
            }
        }
        if !text.is_empty() && (line[range.clone()] != text || cursor != range.start + text.len()) {
            return Some(Completion::Replace(range, text));
        }
    }
    Some(Completion::Matches(path_range))
}

/// Offer replacements for the path component at a UTF-8 byte cursor.
///
/// Each callback receives a byte range in `path` and replacement text. Later
/// components remain untouched; no trailing separator is added. Candidates describe
/// schema possibilities, not runtime presence or writability. Invalid parent paths or
/// cursor positions yield none. Components exceeding `MAX_PATH_LENGTH` bytes
/// are omitted.
/// A valid numeric index offers only that index; an empty component offers all indices.
/// Replacement text is borrowed only for the callback; no allocator is needed.
pub fn complete_path(
    schema: &'static Schema,
    path: &str,
    cursor: usize,
    mut candidate: impl FnMut(Range<usize>, &str),
) {
    if path.is_empty() && cursor == 0 && !schema.is_leaf() {
        candidate(0..0, "/");
        return;
    }
    let Some((range, matches)) = Candidates::path(schema, path, cursor) else {
        return;
    };
    for (text, _) in matches.iter() {
        candidate(range.clone(), &text);
    }
}
