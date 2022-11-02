use miniconf::Miniconf;

#[derive(Debug, Default, Miniconf)]
struct AdditionalSettings {
    inner: u8,
    inner2: u32,
}

#[derive(Debug, Default, Miniconf)]
struct Settings {
    #[miniconf(defer)]
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
    let mut settings_iter = Settings::iter_paths::<5, 128>().unwrap();

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

    for topic in settings_iter {
        let mut value = [0; 256];
        let len = s.get(&topic, &mut value).unwrap();
        println!(
            "{:?}: {:?}",
            topic,
            std::str::from_utf8(&value[..len]).unwrap()
        );
    }
}
