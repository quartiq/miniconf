// use miniconf::{Error, Miniconf};
use serde_json_core::heapless::String;
use miniconf::Miniconf;
use serde::{Serialize,Deserialize};
// use field_offset::offset_of;
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

trait MiniconfIter {
    // default implementation is the base case for primitives where it will
    // yield once for self, then return None on subsequent calls. Structs should
    // implement this method if they should be recursed.
    fn recursive_iter(&self, index: &mut [usize], _topic: &mut String<serde_json_core::heapless::consts::U128>) -> Option<String<serde_json_core::heapless::consts::U128>>
    where Self: serde::Serialize 
    {
        if index.len() == 0 {
            // I don't expect this to happen...
            unreachable!();
            // return None;
        }

        let result = match index[0]
        {
            0 => Some(serde_json_core::to_string(&self).unwrap()),
            _ => None,
        };

        index[0] += 1;
        index[1..].iter_mut().for_each(|x| *x = 0);

        result
    }
}

impl MiniconfIter for u32 { }
impl MiniconfIter for u8 { }

#[derive(Debug, Default, Miniconf, Serialize, Deserialize)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

impl MiniconfIter for AdditionalSettings {
    fn recursive_iter(&self, index: &mut [usize], topic: &mut String<serde_json_core::heapless::consts::U128>) -> Option<String<serde_json_core::heapless::consts::U128>> {
        loop {
            match index[0] {
                0 => {
                    topic.push_str("/inner").unwrap();
                    if let Some(r) = self.inner.recursive_iter(&mut index[1..], topic) {
                        // recursive iterator yielded a string, return it
                        return Some(r);
                    }
                    else
                    {
                        //we're done recursively exploring this field, move to the next
                        index[0] += 1;
                        // reset the state of all following indices
                        index[1..].iter_mut().for_each(|x| *x = 0);
                    }
                }
                1 => {
                    topic.push_str("/inner2").unwrap();
                    if let Some(r) = self.inner2.recursive_iter(&mut index[1..], topic) {
                        // recursive iterator yielded a string, return it
                        return Some(r);
                    }
                    else
                    {
                        //we're done recursively exploring this field, move to the next
                        index[0] += 1;
                        // reset the state of all following indices
                        index[1..].iter_mut().for_each(|x| *x = 0);
                    }
                }
                _ => return None,
            };
        }
    }
}

#[derive(Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

const STACK_SIZE: usize = 3;
pub struct SettingsIter {
    settings: Settings,
    index: [usize; STACK_SIZE],
    // index: usize,
}

impl Iterator for SettingsIter{
    type Item = (String<serde_json_core::heapless::consts::U128>, String<serde_json_core::heapless::consts::U128>);
    fn next(&mut self) -> Option<Self::Item> {
        let mut topic: String<serde_json_core::heapless::consts::U128> = String::new();
        loop {
            match self.index[0] {
                0 => {
                    topic.push_str("/data").unwrap();
                    if let Some(r) = self.settings.data.recursive_iter(&mut self.index[1..], &mut topic) {
                        // recursive iterator yielded a string, return it
                        return Some((topic, r));
                    }
                    else
                    {
                        //we're done recursively exploring this field, move to the next
                        self.index[0] += 1;
                        // reset the state of all following indices
                        self.index[1..].iter_mut().for_each(|x| *x = 0);
                    }
                }
                1 => {
                    topic.push_str("/more").unwrap();
                    if let Some(r) = self.settings.more.recursive_iter(&mut self.index[1..], &mut topic) {
                        // recursive iterator yielded a string, return it
                        return Some((topic,r));
                    }
                    else
                    {
                        //we're done recursively exploring this field, move to the next
                        self.index[0] += 1;
                        // reset the state of all following indices
                        self.index[1..].iter_mut().for_each(|x| *x = 0);
                    }
                }
                _ => return None,
            };
        }
    }
}

fn main() {
    let s = Settings {
        data: 1,
        more: AdditionalSettings {
            inner: 5,
            inner2: 7,
        },
    };

    let i = SettingsIter {
        settings: s,
        index: [0; STACK_SIZE],
    };

    for (topic, value) in i {
        println!("{} {}", topic, value);
    }

}
