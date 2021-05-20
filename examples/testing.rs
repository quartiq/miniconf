// use miniconf::{Error, Miniconf};
use miniconf::{Miniconf};
use serde::Deserialize;

#[derive(Debug, Default, Miniconf, Deserialize)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

impl AdditionalSettings {
    fn string_get(
        &self,
        mut topic_parts: core::iter::Peekable<core::str::Split<&str>>
    ) -> Result<String, ()> {
        let field = topic_parts.next().ok_or(())?;

        match field {
            "inner" => Ok(self.inner.to_string()),
            "inner2" => Ok(self.inner2.to_string()),
            _ => Err(())
        }
    }
}

#[derive(Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

impl Settings {
    fn string_get(
        &self,
        mut topic_parts: core::iter::Peekable<core::str::Split<&str>>,
    ) -> Result<String, ()> {
        let field = topic_parts.next().ok_or(())?;

        match field {
            "data" => Ok(self.data.to_string()),
            "more" => self.more.string_get(topic_parts),
            _ => Err(())
        }
    }
}

fn main() {
    let s = Settings {
        data: 1,
        more: AdditionalSettings{inner: 5, inner2: 7},
    };

    dbg!(s.string_get("data".split("/").peekable()));
    dbg!(s.string_get("more/inner".split("/").peekable()));
    dbg!(s.string_get("more/inner2".split("/").peekable()));

    // for (field, value) in s {
    //     println!("{}: {}", field, value);
    // }

}