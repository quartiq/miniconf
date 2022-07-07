use miniconf::Miniconf;

#[derive(Copy, Clone, Default, Miniconf)]
struct Settings {
    value: Option<u32>,
}

#[test]
fn get_set_none() {
    let mut settings = Settings::default();
    let mut data = [0; 100];

    // Check that if the option is None, the value cannot be get or set.
    settings.value.take();
    assert!(settings.get("value", &mut data).is_err());
    assert!(settings.set("value", b"5").is_err());
}

#[test]
fn get_set_some() {
    let mut settings = Settings::default();
    let mut data = [0; 10];

    // Check that if the option is Some, the value can be get or set.
    settings.value.replace(5);

    let len = settings.get("value", &mut data).unwrap();
    assert_eq!(&data[..len], b"5");

    settings.set("value", b"7").unwrap();
    assert_eq!(settings.value.unwrap(), 7);
}

#[test]
fn iterate_some_none() {
    let mut settings = Settings::default();

    // When the value is None, it should not be iterated over as a topic.
    let mut state = [0; 10];
    settings.value.take();
    let mut iterator = settings.iter_settings::<128>(&mut state).unwrap();
    assert!(iterator.next().is_none());

    // When the value is Some, it should be iterated over.
    let mut state = [0; 10];
    settings.value.replace(5);
    let mut iterator = settings.iter_settings::<128>(&mut state).unwrap();
    assert_eq!(iterator.next().unwrap(), "value");
}
