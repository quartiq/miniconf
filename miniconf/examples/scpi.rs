use core::str;

use miniconf::{
    json, IntoKeys, Keys, Path, PathIter, SerdeError, TreeDeserializeOwned, TreeSchema,
    TreeSerialize, ValueError,
};

mod common;
use common::Settings;

/// This show-cases the implementation of a custom [`miniconf::Key`]
/// along the lines of SCPI style hierarchies. It is case-insensitive and
/// distinguishes relative/absolute paths.
/// It then proceeds to implement a naive SCPI command parser that supports
/// setting and getting values.
///
/// This is just a sketch.

#[derive(Copy, Clone)]
struct ScpiKey<T: ?Sized>(T);

impl<T: AsRef<str> + ?Sized> miniconf::Key for ScpiKey<T> {
    fn find(&self, lookup: &miniconf::Internal) -> Option<usize> {
        use miniconf::Internal::*;
        let s = self.0.as_ref();
        match lookup {
            Named(n) => {
                let mut truncated = None;
                let mut ambiguous = false;
                for (i, miniconf::Named { name, .. }) in n.iter().enumerate() {
                    if name.len() < s.len()
                        || !name
                            .chars()
                            .zip(s.chars())
                            .all(|(n, s)| n.to_ascii_lowercase() == s.to_ascii_lowercase())
                    {
                        continue;
                    }
                    if name.len() == s.len() {
                        // Exact match: return immediately
                        return Some(i);
                    }
                    if truncated.is_some() {
                        // Multiple truncated matches: ambiguous unless there is an additional exact match
                        ambiguous = true;
                    } else {
                        // First truncated match: fine if there is only one.
                        truncated = Some(i);
                    }
                }
                if ambiguous {
                    None
                } else {
                    truncated
                }
            }
            Numbered(n) => s.parse().ok().filter(|i| *i < n.len()),
            Homogeneous(h) => s.parse().ok().filter(|i| *i < h.len.get()),
        }
    }
}

#[derive(Clone)]
struct ScpiPathIter<'a>(PathIter<'a, ':'>);

impl<'a> Iterator for ScpiPathIter<'a> {
    type Item = ScpiKey<&'a str>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(ScpiKey)
    }
}

#[derive(Copy, Clone)]
struct ScpiPath<'a>(Option<&'a str>);

impl<'a> IntoIterator for ScpiPath<'a> {
    type IntoIter = ScpiPathIter<'a>;
    type Item = ScpiKey<&'a str>;
    fn into_iter(self) -> Self::IntoIter {
        ScpiPathIter(PathIter::new(self.0))
    }
}

#[derive(thiserror::Error, Debug, Copy, Clone)]
enum Error {
    #[error("While setting value")]
    Set(#[from] miniconf::SerdeError<serde_json_core::de::Error>),
    #[error("While getting value")]
    Get(#[from] miniconf::SerdeError<serde_json_core::ser::Error>),
    #[error("Parse failure: {0}")]
    Parse(&'static str),
    #[error("Could not print value")]
    Utf8(#[from] core::str::Utf8Error),
}

fn scpi<M: TreeSerialize + TreeDeserializeOwned>(target: &mut M, cmds: &str) -> Result<(), Error> {
    let mut buf = vec![0; 1024];
    let root = ScpiPath(None);
    let mut abs = root;
    for cmd in cmds.split_terminator(';').map(|cmd| cmd.trim()) {
        let (path, value) = if let Some(path) = cmd.strip_suffix('?') {
            (path, None)
        } else if let Some((path, value)) = cmd.split_once(' ') {
            (path, Some(value))
        } else {
            Err(Error::Parse("Missing `?` to get or value to set"))?
        };
        let rel;
        (abs, rel) = if let Some(path) = path.strip_prefix(':') {
            path.rsplit_once(':')
                .map(|(a, r)| (ScpiPath(Some(a)), ScpiPath(Some(r))))
                .unwrap_or((root, ScpiPath(Some(path))))
        } else {
            (abs, ScpiPath(Some(path)))
        };
        let path = abs.into_keys().chain(rel);
        if let Some(value) = value {
            json::set_by_key(target, path, value.as_bytes())?;
            println!("OK");
        } else {
            let len = json::get_by_key(target, path, &mut buf[..])?;
            println!("{}", str::from_utf8(&buf[..len])?);
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut settings = Settings::new();

    scpi(
        &mut settings,
        "fO?; foo?; FOO?; :FOO?; :ARRAY_OPT:1:A?; A?; A?; A 1; A?; :FOO?",
    )?;
    scpi(&mut settings, "FO?; STRUCT_TREE:B 3; STRUCT_TREE:B?")?;

    scpi(&mut settings, ":STRUCT_ 42").unwrap_err();

    let mut buf = vec![0; 1024];
    const MAX_DEPTH: usize = Settings::SCHEMA.shape().max_depth;
    for path in Settings::SCHEMA.nodes::<Path<String, ':'>, MAX_DEPTH>() {
        let path = path?;
        match json::get_by_key(&settings, &path, &mut buf) {
            Ok(len) => println!(
                "{} {}",
                path.0.to_uppercase(),
                core::str::from_utf8(&buf[..len])?
            ),
            Err(SerdeError::Value(ValueError::Absent)) => {
                continue;
            }
            err => {
                err?;
            }
        }
    }
    Ok(())
}
