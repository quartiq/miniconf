// use miniconf::{Error, Miniconf};
use serde_json_core::heapless::String;
use miniconf::Miniconf;
use serde::{Serialize,Deserialize};

#[derive(Debug, Default, Miniconf, Serialize, Deserialize)]
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

#[derive(Debug, Default, Miniconf, Serialize, Deserialize)]
struct Settings {
    more: AdditionalSettings,
    data: u32,
}

impl Settings {
    fn into_miniconf_iter<'a>(self, index_stack: &'a mut [usize],
        topic_buffer: &'a mut String<128>,
        value_buffer: &'a mut String<128>
        ) -> SettingsIter<'a> {
        SettingsIter {
            settings: self,
            index: index_stack,
            topic_buffer,
            value_buffer,
        }
    }
}

pub struct SettingsIter<'a> {
    settings: Settings,
    index: &'a mut [usize],
    topic_buffer: &'a mut String<128>,
    value_buffer: &'a mut String<128>,
}

impl<'a> Iterator for SettingsIter<'a>{
    type Item = (String<128>, String<128>);
    fn next(&mut self) -> Option<(String<128>, String<128>)> {
        self.topic_buffer.clear();
        if let Some(()) = self.settings.recursive_iter(&mut self.index, &mut self.topic_buffer, &mut self.value_buffer) {
            Some((self.topic_buffer.clone(), self.value_buffer.clone()))
        }
        else {
            None
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

    let mut index_stack = [0; 1];
    let mut topic_buffer = String::new();
    let mut value_buffer = String::new();

    for (topic, value) in s.into_miniconf_iter(&mut index_stack, &mut topic_buffer, &mut value_buffer) {
        println!("{} {}", topic, value);
    }

}
