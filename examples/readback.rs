// use miniconf::{Error, Miniconf};
use miniconf::Miniconf;
use serde::{Deserialize, Serialize};

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

    let mut settings_iter = s.into_iter::<128>(&mut iterator_state).unwrap();

    // Just get one topic/value from the iterator
    if let Some(topic) = settings_iter.next() {
        let mut value = [0; 256];
        let len = s.get(&topic, &mut value).unwrap();
        println!(
            "{:?}: {:?}",
            topic,
            std::str::from_utf8(&value[..len]).unwrap()
        );
    }

    // Modify settings data, proving iterator is out of scope and has released
    // the settings
    s.data = 3;

    // Create a new settings iterator, print remaining values
    for topic in s.into_iter::<128>(&mut iterator_state).unwrap() {
        let mut value = [0; 256];
        let len = s.get(&topic, &mut value).unwrap();
        println!(
            "{:?}: {:?}",
            topic,
            std::str::from_utf8(&value[..len]).unwrap()
        );
    }
}
