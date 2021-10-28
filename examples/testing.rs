// use miniconf::{Error, Miniconf};
use serde_json_core::heapless::String;
use miniconf::Miniconf;
use serde::{Serialize,Deserialize};
use field_offset::offset_of;
// Let's build an array that contains a tuple of the settings topics (since they
// all need to be stored in flash anyways, might as well put them in an iterable
// structure). While we're at it, we can also store the offset of the setting in
// memory for quick lookups? Can we do this safely? If not, recursive
// string_get() should work. Perhaps we can safely get a reference.
// https://crates.io/crates/field-offset looks good for storing struct offsets
// and using them later. When we implement the iterator for the struct, it can
// use this array of topic names and offsets for accessing the underlying
// settings.

// Theoretically we could store all of the topic strings in a suffix trie like
// structure instead of an array to save memory. We could perform this
// optimization down the road if needed.

#[derive(Debug, Default, Miniconf, Serialize, Deserialize)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

// impl<'a> IntoIterator

#[derive(Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

struct SettingsIterRow<'a, T> {
    topic: &'a str,
    value_closure: fn(&'a T) -> String<serde_json_core::heapless::consts::U128>,
}

pub struct SettingsIter {
    settings: Settings,
    index: usize,
}

impl Iterator for SettingsIter{
    type Item = (&'static str, String<serde_json_core::heapless::consts::U128>);
    fn next(&mut self) -> Option<Self::Item> {
        let result = match self.index {
            0 => ("/data", serde_json_core::to_string(&self.settings.data).unwrap()),
            1 => ("/more/inner", serde_json_core::to_string(&self.settings.more.inner).unwrap()),
            2 => ("/more/inner2", serde_json_core::to_string(&self.settings.more.inner2).unwrap()),
            _ => return None,
        };
        self.index += 1;
        Some(result)
    }
}

macro_rules! offset_entry {
    ($t : expr ) => { 
        |settings_struct| -> String<serde_json_core::heapless::consts::U128> {
            let b = field_offset::offset_of!($t);
            serde_json_core::to_string(b.apply(settings_struct)).unwrap()
        }
    };
}

fn main() {
    let s = Settings {
        data: 1,
        more: AdditionalSettings {
            inner: 5,
            inner2: 7,
        },
    };

    // let i = SettingsIter {
    //     settings: s,
    //     index: 0,
    // };

    // for mstr in i {
    //     println!("{} {}", mstr.0, mstr.1);
    // }

    // Idea: struct will store an array of tuples of topic string as well as a
    // lambda to compute the string of the field given the main settings struct.
    // We might be able to hide all the type information behind the lambda,
    // giving the lamda signature: fn x(T) -> &str

    // let data_closure = offset_entry!(Settings=>data);

    // let data_closure = |settings_struct| -> String<serde_json_core::heapless::consts::U128> {
    //     let b = field_offset::offset_of!(Settings=>data);
    //     serde_json_core::to_string(b.apply(settings_struct)).unwrap()
    // };

    let inner_closure = |settings_struct| -> String<serde_json_core::heapless::consts::U128> {
        let offset = field_offset::offset_of!(Settings=>more: AdditionalSettings=>inner);
        serde_json_core::to_string(offset.apply(settings_struct)).unwrap()
    };

    let iter_table: [SettingsIterRow<Settings>; 2] = [
        SettingsIterRow{topic: "more/inner", value_closure: inner_closure },
        SettingsIterRow{topic: "data", value_closure: data_closure } 
    ];

    for row in iter_table {
        println!("{} {}", row.topic, (row.value_closure)(&s));
    }
}
