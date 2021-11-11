// use miniconf::{Error, Miniconf};
use miniconf::Miniconf;
use serde::{Deserialize, Serialize};
use serde_json_core::heapless::String;

#[derive(Debug, Default, Miniconf, Serialize, Deserialize)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

#[derive(Debug, Default, Miniconf, Serialize, Deserialize)]
struct Settings {
    more: AdditionalSettings,
    data: u32,
}

// This will eventually be a derived impl
impl Settings {
    fn miniconf_iter<'a, 'b, const TS: usize, const VS: usize>(
        &'b self,
        index_stack: &'a mut [usize],
    ) -> SettingsMiniconfIter<'a, 'b, TS, VS> {
        SettingsMiniconfIter {
            settings: &self,
            index: index_stack,
        }
    }
}

// This will eventually be derived
// TS is the size of the topic buffer
// VS is the size of the value buffer
pub struct SettingsMiniconfIter<'a, 'b, const TS: usize, const VS: usize> {
    settings: &'b Settings,
    index: &'a mut [usize],
}

// This will eventually be derived
impl<'a, const TS: usize, const VS: usize> Iterator for SettingsMiniconfIter<'a, '_, TS, VS> {
    type Item = (String<TS>, String<VS>);
    fn next(&mut self) -> Option<Self::Item> {
        let mut topic_buffer: String<TS> = String::new();
        let mut value_buffer: String<VS> = String::new();
        topic_buffer.clear();
        value_buffer.clear();
        if let Some(()) = self.settings.recursive_iter::<TS, VS>(
            &mut self.index,
            &mut topic_buffer,
            &mut value_buffer,
        ) {
            Some((topic_buffer, value_buffer))
        } else {
            None
        }
    }
}

// Possible alternative to implementing Iterator?
pub struct IterState<T: Miniconf + Serialize, const S: usize> {
    index: [usize; S],
    _type: core::marker::PhantomData<T>,
}

impl<'a, T: Miniconf + Serialize, const S: usize> IterState<T, S> {
    fn new() -> IterState<T, S> {
        IterState {
            index: [0; S],
            _type: core::marker::PhantomData::<T>,
        }
    }

    fn reset(&mut self) {
        self.index = [0; S];
    }

    fn next<const TS: usize, const VS: usize>(
        &mut self,
        settings: &T,
        topic_buffer: &'a mut String<TS>,
        value_buffer: &'a mut String<VS>,
    ) -> Option<(&'a String<TS>, &'a String<VS>)> {
        topic_buffer.clear();
        value_buffer.clear();
        if let Some(()) =
            settings.recursive_iter::<TS, VS>(&mut self.index, topic_buffer, value_buffer)
        {
            Some((topic_buffer, value_buffer))
        } else {
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

    // Maintains our state of iteration. This is created external from the
    // iterator struct so that we can destroy the iterator struct, create a new
    // one, and resume from where we left off.
    // Perhaps we can wrap this up as some sort of templated `MiniconfIterState`
    // type? That way we can hide what it is.
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

    println!("\nAlternative Implementation:");
    let mut value_buffer: String<128> = String::new();
    let mut topic_buffer: String<128> = String::new();

    let mut iter_state: IterState<Settings, 5> = IterState::new();

    loop {
        if let Some((topic, value)) = iter_state.next(&s, &mut topic_buffer, &mut value_buffer) {
            println!("{} {}", topic, value);
        } else {
            // done iterating, break out
            break;
        }

        // Proving that settings can be modified between iteration calls
        s.data = s.data.wrapping_add(1);
    }
}
