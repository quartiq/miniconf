use core::{fmt::Write as _, ops::Range};

use heapless::String;
use miniconf::{Internal, Schema};

use crate::MAX_PATH_LENGTH;

pub(super) enum Candidates<'a> {
    Root,
    Commands(&'a [&'a str], &'a str),
    Children(&'static Internal, &'a str),
}

impl<'a> Candidates<'a> {
    pub(super) fn path(
        schema: &'static Schema,
        path: &'a str,
        cursor: usize,
    ) -> Option<(Range<usize>, Self)> {
        if path.is_empty() && cursor == 0 && !schema.is_leaf() {
            return Some((0..0, Self::Root));
        }
        let before = path.get(..cursor)?;
        let (parent, prefix) = before.rsplit_once('/')?;
        let children = schema.get(parent).ok()?.schema.internal()?;
        let end = cursor + path[cursor..].find('/').unwrap_or(path.len() - cursor);
        Some((
            before.len() - prefix.len()..end,
            Self::Children(children, prefix),
        ))
    }

    pub(super) fn numeric_len(&self) -> Option<usize> {
        match self {
            Self::Children(children, _) if !matches!(children, Internal::Named(_)) => {
                Some(children.len().get())
            }
            _ => None,
        }
    }

    fn iter(&self) -> impl Iterator<Item = (String<MAX_PATH_LENGTH>, bool)> + Clone {
        let (len, mut prefix) = match self {
            Self::Root => (1, ""),
            Self::Commands(names, prefix) => (names.len(), *prefix),
            Self::Children(children, prefix) => (children.len().get(), *prefix),
        };
        let indices = if self.numeric_len().is_some() && !prefix.is_empty() {
            let index = prefix.parse::<usize>().ok().filter(|&index| index < len);
            prefix = "";
            index.map_or(0..0, |index| index..index + 1)
        } else {
            0..len
        };
        indices.filter_map(move |index| {
            let mut name = String::new();
            let branch = match self {
                Self::Root => {
                    name.push('/').ok()?;
                    true
                }
                Self::Commands(names, _) => {
                    name.push_str(names[index]).ok()?;
                    false
                }
                Self::Children(children, _) => {
                    match children.get_name(index) {
                        Some(text) => name.push_str(text).ok()?,
                        None => write!(name, "{index}").ok()?,
                    }
                    !children.get_schema(index).is_leaf()
                }
            };
            name.starts_with(prefix).then_some((name, branch))
        })
    }

    pub(super) fn tokens(&self) -> impl Iterator<Item = (String<MAX_PATH_LENGTH>, bool)> + Clone {
        self.iter()
            .filter(|(name, _)| !name.chars().any(|ch| ch.is_whitespace() || ch.is_control()))
    }
}

pub(super) struct Context<'a> {
    range: Range<usize>,
    path: Option<Range<usize>>,
    pub(super) candidates: Candidates<'a>,
}

impl<'a> Context<'a> {
    pub(super) fn new(
        schema: &'static Schema,
        line: &'a str,
        cursor: usize,
        commands: &'a [&'a str],
    ) -> Option<Self> {
        let before = line.get(..cursor)?;
        let start = line.len() - line.trim_start().len();
        let end = start + line[start..].split_whitespace().next().unwrap_or("").len();
        if cursor < start {
            return None;
        }
        if cursor <= end {
            return Some(Self {
                range: start..end,
                path: None,
                candidates: Candidates::Commands(commands, &before[start..]),
            });
        }
        if !commands.contains(&&line[start..end])
            || !matches!(&line[start..end], "get" | "set" | "schema")
        {
            return None;
        }
        let args = end + line[end..].len() - line[end..].trim_start().len();
        if cursor < args || line[args..cursor].contains(char::is_whitespace) {
            return None;
        }
        let path_end = args
            + line[args..]
                .find(char::is_whitespace)
                .unwrap_or(line.len() - args);
        let (range, candidates) = Candidates::path(schema, &line[args..path_end], cursor - args)?;
        Some(Self {
            range: args + range.start..args + range.end,
            path: Some(args..cursor),
            candidates,
        })
    }
}

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
    let Context {
        mut range,
        path,
        candidates,
    } = Context::new(schema, line, cursor, commands)?;
    let limit = if candidates.numeric_len().is_some() {
        2
    } else {
        usize::MAX
    };
    let mut names = candidates.tokens().take(limit);
    let Some((mut text, branch)) = names.next() else {
        return Some(Completion::Matches(path));
    };
    let mut unique = true;
    for (other, _) in names {
        unique &= text == other;
        let len = text
            .chars()
            .zip(other.chars())
            .take_while(|(a, b)| a == b)
            .map(|(c, _)| c.len_utf8())
            .sum();
        text.truncate(len);
        if !unique && text.is_empty() {
            break;
        }
    }
    if unique && path.is_none() && range.end == line.len() {
        text.push(' ').ok()?;
    }
    if unique && path.is_some() && !branch && !line[range.end..].starts_with('/') {
        text.push(' ').ok()?;
        if let Some(space) = line[range.end..].chars().next() {
            range.end += space.len_utf8();
        }
    }
    if !text.is_empty() && (line[range.clone()] != text || cursor != range.start + text.len()) {
        return Some(Completion::Replace(range, text));
    }
    Some(Completion::Matches(path))
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
    let Some((range, matches)) = Candidates::path(schema, path, cursor) else {
        return;
    };
    for (text, _) in matches.iter() {
        candidate(range.clone(), &text);
    }
}
