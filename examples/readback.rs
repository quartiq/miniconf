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
    fn miniconf_iter<'a, 'b, const TS: usize, const VS: usize>(&'b self, index_stack: &'a mut [usize],
        ) -> SettingsIter<'a, 'b, TS, VS> {
        SettingsIter {
            settings: &self,
            index: index_stack,
        }
    }
}

pub struct SettingsIter<'a, 'b, const TS: usize, const VS: usize> {
    settings: &'b Settings,
    index: &'a mut [usize],
}

impl<'a, const TS: usize, const VS: usize> Iterator for SettingsIter<'a, '_, TS, VS>{
    type Item = (String<TS>, String<VS>);
    fn next(&mut self) -> Option<(String<TS>, String<VS>)> {
        let mut topic_buffer: String<TS> = String::new();
        let mut value_buffer: String<VS> = String::new();
        topic_buffer.clear();
        if let Some(()) = self.settings.recursive_iter::<TS, VS>(&mut self.index, &mut topic_buffer, &mut value_buffer) {
            Some((topic_buffer, value_buffer))
        }
        else {
            None
        }
    }
}

fn main() {
    let mut s = Settings {
        data: 1,
        more: AdditionalSettings {
            inner: 5,
            inner2: 7,
        },
    };

    // Maintains our state of iteration
    let mut iterator_state = [0; 5];

    let mut settings_iter = s.miniconf_iter::<128, 10>(&mut iterator_state);

    // Just get one topic/value from the iterator
    if let Some((topic, value)) = settings_iter.next() {
        println!("{} {}", topic, value);
    }

    // Modify settings data, proving iterator is out of scope and has released
    // the settings
    s.data = 3;

    // Create a new settings iterator, print remaining values
    let settings_iter = s.miniconf_iter::<128, 10>(&mut iterator_state);
    for (topic, value) in settings_iter {
        println!("{} {}", topic, value);
    }
}
