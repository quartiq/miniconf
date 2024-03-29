use miniconf::{JsonCoreSlash, Tree, TreeKey};

#[derive(Debug, Default, Tree)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

#[derive(Debug, Default, Tree)]
struct Settings {
    #[tree]
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

    // Maintains our state of iteration.
    let mut settings_iter = Settings::iter_paths::<String>("/");

    // Just get one topic/value from the iterator
    if let Some(topic) = settings_iter.next() {
        let topic = topic.unwrap();
        let mut value = [0; 256];
        let len = s.get_json(&topic, &mut value).unwrap();
        println!(
            "{:?}: {:?}",
            topic,
            std::str::from_utf8(&value[..len]).unwrap()
        );
    }

    // Modify settings data, proving iterator is out of scope and has released
    // the settings
    s.data = 3;

    for topic in settings_iter {
        let topic = topic.unwrap();
        let mut value = [0; 256];
        let len = s.get_json(&topic, &mut value).unwrap();
        println!(
            "{:?}: {:?}",
            topic,
            std::str::from_utf8(&value[..len]).unwrap()
        );
    }
}
