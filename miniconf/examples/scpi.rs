use core::str;

use miniconf::{
    json, IntoKeys, Key, KeyLookup, Keys, PathIter, TreeDeserializeOwned, TreeSerialize,
};

mod common;
use common::Settings;

/// This show-cases the implementation of a custom [`miniconf::Key`]
/// alogn the lines of SCPI style hierarchies.
/// It then proceeds to implement a SCPI command parser that supports
/// setting and getting, case-insensitive, and distinguishes relative/absolute
/// paths.
///
/// This is just a sketch. There is no error handling.

#[derive(Copy, Clone)]
struct ScpiKey<T: ?Sized>(T);

impl<T: AsRef<str> + ?Sized> Key for ScpiKey<T> {
    fn find(&self, lookup: &KeyLookup) -> Option<usize> {
        let s = self.0.as_ref();
        match lookup.names {
            Some(names) => {
                let mut truncated = None;
                let mut ambiguous = false;
                for (i, name) in names.iter().enumerate() {
                    if name.len() < s.len() {
                        continue;
                    }
                    if name
                        .chars()
                        .zip(s.chars())
                        .all(|(n, s)| n.to_ascii_lowercase() == s.to_ascii_lowercase())
                    {
                        if name.len() == s.len() {
                            // Exact match: return immediately
                            return Some(i);
                        }
                        if truncated.is_some() {
                            // Multiple truncated matches: ambiguous if there isn't an additional exact match
                            ambiguous = true;
                        } else {
                            // First truncated match: fine if there is only one.
                            truncated = Some(i);
                        }
                    }
                }
                if ambiguous {
                    None
                } else {
                    truncated
                }
            }
            None => s.parse().ok(),
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

impl<'a> IntoIterator for &'a ScpiPath<'a> {
    type IntoIter = ScpiPathIter<'a>;
    type Item = ScpiKey<&'a str>;
    fn into_iter(self) -> Self::IntoIter {
        ScpiPathIter(PathIter::new(self.0))
    }
}

struct ScpiCtrl<M, const Y: usize> {
    settings: M,
    buf: [u8; 1024],
}

impl<M: TreeSerialize<Y> + TreeDeserializeOwned<Y>, const Y: usize> ScpiCtrl<M, Y> {
    fn new(settings: M) -> Self {
        Self {
            settings,
            buf: [0; 1024],
        }
    }

    fn get(&mut self, path: impl IntoKeys) -> &str {
        let len = json::get_by_key(&self.settings, path, &mut self.buf[..]).unwrap();
        str::from_utf8(&self.buf[..len]).unwrap()
    }

    fn set(&mut self, path: impl IntoKeys, value: &str) {
        json::set_by_key(&mut self.settings, path, value.as_bytes()).unwrap();
    }

    fn cmd(&mut self, cmd: &str) {
        let root = ScpiPath(None);
        let mut abs = root;
        let mut rel;
        for mut cmd in cmd.split_terminator(';') {
            cmd = cmd.trim();
            let (path, value) = if let Some(path) = cmd.strip_suffix('?') {
                (path, None)
            } else if let Some((path, value)) = cmd.split_once(' ') {
                (path, Some(value))
            } else {
                println!("Could not parse: {}", cmd);
                continue;
            };
            if let Some(path) = path.strip_prefix(':') {
                if let Some((abs_path, rel_path)) = path.rsplit_once(':') {
                    abs = ScpiPath(Some(abs_path));
                    rel = ScpiPath(Some(rel_path));
                } else {
                    abs = root;
                    rel = ScpiPath(Some(path));
                }
            } else {
                rel = ScpiPath(Some(path));
            };
            let path = Keys::chain(abs.into_keys(), &rel);
            if let Some(value) = value {
                self.set(path, value);
                println!("OK");
            } else {
                println!("{}", self.get(path));
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut settings = Settings::default();
    settings.enable();
    let mut ctrl = ScpiCtrl::new(settings);

    ctrl.set(&ScpiPath(Some("ARRAY_OPT:1:A")), "99");
    println!("{}", ctrl.get(&ScpiPath(Some("ArrAY_opTION_TRE:1:A"))));

    ctrl.cmd("FOO?; :ARRAY_OPT:1:A?; A?; A?; A 1; A?; :FOO?");
    ctrl.cmd("FO?; STRUCT_TREE:B 3; STRUCT_TREE:B?");

    Ok(())
}
