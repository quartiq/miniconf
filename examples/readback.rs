// use miniconf::{Error, Miniconf};
use serde_json_core::heapless::String;
use miniconf::Miniconf;
use serde::{Serialize,Deserialize};

use miniconf::MiniconfIter;


#[derive(Debug, Default, Miniconf, MiniconfIter, Serialize, Deserialize)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

// impl MiniconfIter for AdditionalSettings {
//     fn recursive_iter(&self, index: &mut [usize], topic: &mut String<128>) -> Option<String<128>> {
//         loop {
//             match index[0] {
//                 0 => {
//                     topic.push_str("/inner").unwrap();
//                     if let Some(r) = self.inner.recursive_iter(&mut index[1..], topic) {
//                         // recursive iterator yielded a string, return it
//                         return Some(r);
//                     }
//                     else
//                     {
//                         //we're done recursively exploring this field, move to the next
//                         index[0] += 1;
//                         // reset the state of all following indices
//                         index[1..].iter_mut().for_each(|x| *x = 0);
//                     }
//                 }
//                 1 => {
//                     topic.push_str("/inner2").unwrap();
//                     if let Some(r) = self.inner2.recursive_iter(&mut index[1..], topic) {
//                         // recursive iterator yielded a string, return it
//                         return Some(r);
//                     }
//                     else
//                     {
//                         //we're done recursively exploring this field, move to the next
//                         index[0] += 1;
//                         // reset the state of all following indices
//                         index[1..].iter_mut().for_each(|x| *x = 0);
//                     }
//                 }
//                 _ => return None,
//             };
//         }
//     }
// }

#[derive(Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

impl Settings {
    fn into_miniconf_iter<'a>(self, index_stack: &'a mut [usize]) -> SettingsIter<'a> {
        SettingsIter {
            settings: self,
            index: index_stack,
        }
    }
}

pub struct SettingsIter<'a> {
    settings: Settings,
    index: &'a mut [usize],
}

impl<'a> Iterator for SettingsIter<'a>{
    type Item = (String<128>, String<128>);
    fn next(&mut self) -> Option<Self::Item> {
        let mut topic: String<128> = String::new();
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

    let mut index_stack = [0; 5];

    for (topic, value) in s.into_miniconf_iter(&mut index_stack) {
        println!("{} {}", topic, value);
    }

}