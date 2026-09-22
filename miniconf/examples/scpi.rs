//! SCPI-style colon headers with case-insensitive, unique-prefix matching.
//! Each call handles one message; relative headers start at the previous parent.
//! Values and replies use JSON, writes print `OK`, and the first error stops the
//! message. Quoted semicolons and SCPI parameter syntax are not supported.

use core::str;

use miniconf::{
    ConstPathIter, Internal, IntoKeys, Key, KeyError, Keys, SerdeError, TreeDeserializeOwned,
    TreeSerialize, json_core,
};

mod common;
use common::Settings;

struct ScpiKey<'a>(&'a str);

impl Key for ScpiKey<'_> {
    fn find(&self, lookup: &Internal) -> Option<usize> {
        use Internal::*;
        let s = self.0;
        match lookup {
            Named(n) => {
                let mut truncated = None;
                let mut ambiguous = false;
                for (i, named) in n.iter().enumerate() {
                    let name = named.name();
                    if !name
                        .as_bytes()
                        .get(..s.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(s.as_bytes()))
                    {
                        continue;
                    }
                    if name.len() == s.len() {
                        return Some(i);
                    }
                    if truncated.is_some() {
                        // Keep looking: a later exact match wins.
                        ambiguous = true;
                    } else {
                        truncated = Some(i);
                    }
                }
                if ambiguous { None } else { truncated }
            }
            Numbered(n) => s.parse().ok().filter(|i| *i < n.len()),
            Homogeneous(h) => s.parse().ok().filter(|i| *i < h.len().get()),
        }
    }
}

struct ScpiPath<'a>(ConstPathIter<'a, ':'>);

impl Keys for ScpiPath<'_> {
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        let key = Iterator::next(&mut self.0).ok_or(KeyError::TooShort)?;
        ScpiKey(key).find(internal).ok_or(KeyError::NotFound)
    }

    fn finalize(&mut self) -> Result<(), KeyError> {
        match Iterator::next(&mut self.0) {
            Some(_) => Err(KeyError::TooLong),
            None => Ok(()),
        }
    }
}

impl IntoKeys for ScpiPath<'_> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

#[derive(thiserror::Error, Debug, Copy, Clone)]
enum Error {
    #[error("While setting value")]
    Set(#[from] SerdeError<serde_json_core::de::Error>),
    #[error("While getting value")]
    Get(#[from] SerdeError<serde_json_core::ser::Error>),
    #[error("Parse failure: {0}")]
    Parse(&'static str),
    #[error("Could not print value")]
    Utf8(#[from] core::str::Utf8Error),
}

fn scpi<M: TreeSerialize + TreeDeserializeOwned>(target: &mut M, cmds: &str) -> Result<(), Error> {
    // Capacities cover this demo tree's headers and JSON leaf values.
    let mut buf = [0; 128];
    let mut path = heapless_09::String::<128>::new();
    for cmd in cmds.split_terminator(';').map(|cmd| cmd.trim()) {
        let (header, value) = cmd.split_once([' ', '\t']).unwrap_or((cmd, ""));
        let query = header.ends_with('?');
        let header = header.strip_suffix('?').unwrap_or(header);
        if query != value.is_empty() {
            return Err(Error::Parse("Expected `?` or a JSON value"));
        }
        let header = if let Some(header) = header.strip_prefix(':') {
            path.clear();
            header
        } else {
            header
        };
        if header.split(':').any(str::is_empty) {
            return Err(Error::Parse("Empty header segment"));
        }
        path.push_str(header)
            .map_err(|_| Error::Parse("Header too long"))?;
        let keys = ScpiPath(ConstPathIter::new(Some(&path)));
        if query {
            let len = json_core::get_by_key(target, keys, &mut buf)?;
            println!("{}", str::from_utf8(&buf[..len])?);
        } else {
            json_core::set_by_key(target, keys, value.as_bytes())?;
            println!("OK");
        }
        path.truncate(path.rfind(':').map_or(0, |i| i + 1));
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut settings = Settings::new();

    scpi(
        &mut settings,
        "CONT:ENAB?; ENAB\tfalse; MODE?; :OUTPUT:DAC:1?; 1 2048; 1?",
    )?;
    assert!(!settings.control.enabled);
    assert_eq!(settings.output.dac[1], 2048);
    assert!(scpi(&mut settings, ":C?").is_err());
    Ok(())
}

#[test]
fn commands() -> anyhow::Result<()> {
    main()?;
    let mut settings = Settings::new();
    scpi(&mut settings, "serial?; output:dac:0 7; 1\t 9;")?;
    assert_eq!(settings.output.dac, [7, 9]);
    scpi(&mut settings, "control:enabled false; enabled?")?;
    assert!(!settings.control.enabled);
    for cmd in [
        ":OUTPUT::DAC:0?",
        ":CONTROL:ENABLED",
        ":CONTROL:ENABLED? false",
    ] {
        assert!(matches!(scpi(&mut settings, cmd), Err(Error::Parse(_))));
    }
    assert!(matches!(
        scpi(&mut settings, &format!("{}?", "x".repeat(129))),
        Err(Error::Parse("Header too long"))
    ));
    assert!(scpi(&mut settings, ":OUTPUT:DAC:0 5000; 1 10").is_err());
    assert_eq!(settings.output.dac, [7, 9]);
    Ok(())
}
