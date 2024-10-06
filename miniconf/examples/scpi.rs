use core::str;

use miniconf::{
    json, IntoKeys, Key, KeyLookup, KeysIter, PathIter, TreeDeserializeOwned, TreeSerialize,
};

mod common;
use common::Settings;

struct ScpiKey<T: ?Sized>(T);

impl<T: AsRef<str> + ?Sized> Key for ScpiKey<T> {
    fn find<M: KeyLookup + ?Sized>(&self) -> Option<usize> {
        let s = self.0.as_ref();
        match M::NAMES {
            Some(names) => {
                let mut idx = [None; 2];
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
                            return Some(i);
                        }
                        if idx[1].is_some() {
                        } else if idx[0].is_some() {
                            idx[1] = Some(i);
                        } else {
                            idx[0] = Some(i);
                        }
                    }
                }
                if idx[1].is_some() {
                    None
                } else {
                    idx[0]
                }
            }
            None => s.parse().ok(),
        }
    }
}

struct ScpiPathIter<'a>(PathIter<'a, ':'>);

impl<'a> Iterator for ScpiPathIter<'a> {
    type Item = ScpiKey<&'a str>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(ScpiKey)
    }
}

struct ScpiPath<T: ?Sized>(T);

impl<'a, T: AsRef<str> + ?Sized> IntoKeys for &'a ScpiPath<T> {
    type IntoKeys = KeysIter<ScpiPathIter<'a>>;
    fn into_keys(self) -> Self::IntoKeys {
        ScpiPathIter(PathIter::new(self.0.as_ref())).into_keys()
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

    fn get(&mut self, path: &str) -> &str {
        let path = ScpiPath(path);
        let len = json::get_by_key(&self.settings, &path, &mut self.buf[..]).unwrap();
        str::from_utf8(&self.buf[..len]).unwrap()
    }

    fn set(&mut self, path: &str, value: &str) {
        let path = ScpiPath(path);
        json::set_by_key(&mut self.settings, &path, value.as_bytes()).unwrap();
    }
}

fn main() -> anyhow::Result<()> {
    let mut settings = Settings::default();
    settings.enable();
    let mut ctrl = ScpiCtrl::new(settings);

    ctrl.set(":ARRAY_OPT:1:A", "99");
    println!("{}", ctrl.get(":ArrAY_opTION_TRE:1:A"));

    Ok(())
}
