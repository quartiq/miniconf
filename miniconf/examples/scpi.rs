use core::str;

use miniconf::{json, IntoKeys, Keys, PathIter, TreeDeserializeOwned, TreeSerialize};

mod common;

/// This show-cases the implementation of a custom [`miniconf::Key`]
/// along the lines of SCPI style hierarchies. It is case-insensitive and
/// distinguishes relative/absolute paths.
/// It then proceeds to implement a SCPI command parser that supports
/// setting and getting.
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
    Set(#[from] miniconf::SerDeError<serde_json_core::de::Error>),
    #[error("While getting value")]
    Get(#[from] miniconf::SerDeError<serde_json_core::ser::Error>),
    #[error("Parse failure: {0}")]
    Parse(&'static str),
    #[error("Could not print value")]
    Utf8(#[from] core::str::Utf8Error),
}

struct ScpiCtrl<M>(M);

impl<M: TreeSerialize + TreeDeserializeOwned> ScpiCtrl<M> {
    fn new(settings: M) -> Self {
        Self(settings)
    }

    fn cmd(&mut self, cmds: &str) -> Result<(), Error> {
        let mut buf = [0; 1024];
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
                json::set_by_key(&mut self.0, path, value.as_bytes())?;
                println!("OK");
            } else {
                let len = json::get_by_key(&self.0, path, &mut buf[..])?;
                println!("{}", str::from_utf8(&buf[..len])?);
            }
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let settings = common::Settings::new();
    let mut ctrl = ScpiCtrl::new(settings);

    ctrl.cmd("fO?; foo?; FOO?; :FOO?; :ARRAY_OPT:1:A?; A?; A?; A 1; A?; :FOO?")?;
    ctrl.cmd("FO?; STRUCT_TREE:B 3; STRUCT_TREE:B?")?;

    ctrl.cmd(":STRUCT_ 42")?;
    Ok(())
}
