use miniconf::{Miniconf, OptionalSetting};

#[derive(PartialEq, Debug, Clone, Default, Miniconf)]
struct Inner {
    data: u32,
}

#[derive(Debug, Clone, Default, Miniconf)]
struct Settings {
    #[miniconf(defer)]
    value: OptionalSetting<Inner>,
}

#[test]
fn option_get_set_none() {
    let mut settings = Settings::default();
    let mut data = [0; 100];

    // Check that if the option is None, the value cannot be get or set.
    settings.value.take();
    assert!(settings.get("value", &mut data).is_err());
    assert!(settings.set("value/data", b"5").is_err());
}

#[test]
fn option_get_set_some() {
    let mut settings = Settings::default();
    let mut data = [0; 10];

    // Check that if the option is Some, the value can be get or set.
    settings.value.replace(Inner { data: 5 });

    let len = settings.get("value/data", &mut data).unwrap();
    assert_eq!(&data[..len], b"5");

    settings.set("value/data", b"7").unwrap();
    assert_eq!(settings.value.as_ref().unwrap().data, 7);
}

#[test]
fn option_iterate_some_none() {
    let mut settings = Settings::default();

    // When the value is None, it should not be iterated over as a topic.
    let mut state = [0; 10];
    settings.value.take();
    let mut iterator = settings.iter_paths::<128>(&mut state).unwrap();
    assert!(iterator.next().is_none());

    // When the value is Some, it should be iterated over.
    let mut state = [0; 10];
    settings.value.replace(Inner { data: 5 });
    let mut iterator = settings.iter_paths::<128>(&mut state).unwrap();
    assert_eq!(iterator.next().unwrap(), "value/data");
}

#[test]
fn option_test_normal_option() {
    #[derive(Copy, Clone, Default, Miniconf)]
    struct S {
        data: Option<u32>,
    }

    let mut s = S::default();
    s.data.take();

    let mut state = [0; 10];
    let mut iterator = s.iter_paths::<128>(&mut state).unwrap();
    assert!(iterator.next().is_some());

    s.set("data", b"7").unwrap();
    assert_eq!(s.data.unwrap(), 7);

    let mut state = [0; 10];
    let mut iterator = s.iter_paths::<128>(&mut state).unwrap();
    assert!(iterator.next().is_some());

    s.set("data", b"null").unwrap();
    assert!(s.data.is_none());
}
