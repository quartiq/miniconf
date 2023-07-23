use miniconf::{Miniconf, MiniconfJson};

#[derive(PartialEq, Debug, Clone, Default, Miniconf)]
struct Inner {
    data: u32,
}

#[derive(Debug, Clone, Default, Miniconf)]
struct Settings {
    #[miniconf(defer)]
    value: miniconf::Option<Inner>,
}

#[test]
fn option_get_set_none() {
    let mut settings = Settings::default();
    let mut data = [0; 100];

    // Check that if the option is None, the value cannot be get or set.
    settings.value.take();
    assert_eq!(
        settings.get("value_foo", &mut data),
        Err(miniconf::Error::PathNotFound)
    );
    assert_eq!(
        settings.get("value", &mut data),
        Err(miniconf::Error::PathAbsent)
    );
    assert_eq!(
        settings.set("value/data", b"5"),
        Err(miniconf::Error::PathAbsent)
    );
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
    assert_eq!((*settings.value).as_ref().unwrap().data, 7);
}

#[test]
fn option_iterate_some_none() {
    let mut settings = Settings::default();

    // When the value is None, it will still be iterated over as a topic but may not exist at runtime.
    settings.value.take();
    let mut iterator = Settings::iter_paths::<10, 128>('/').unwrap();
    assert_eq!(iterator.next().unwrap(), "value/data");
    assert!(iterator.next().is_none());

    // When the value is Some, it should be iterated over.
    settings.value.replace(Inner { data: 5 });
    let mut iterator = Settings::iter_paths::<10, 128>('/').unwrap();
    assert_eq!(iterator.next().unwrap(), "value/data");
    assert!(iterator.next().is_none());
}

#[test]
fn option_test_normal_option() {
    #[derive(Copy, Clone, Default, Miniconf)]
    struct S {
        data: Option<u32>,
    }

    let mut s = S::default();
    assert!(s.data.is_none());

    let mut iterator = S::iter_paths::<10, 128>('/').unwrap();
    assert_eq!(iterator.next(), Some("data".into()));
    assert!(iterator.next().is_none());

    s.set("data", b"7").unwrap();
    assert_eq!(s.data, Some(7));

    let mut iterator = S::iter_paths::<10, 128>('/').unwrap();
    assert_eq!(iterator.next(), Some("data".into()));
    assert!(iterator.next().is_none());

    s.set("data", b"null").unwrap();
    assert!(s.data.is_none());
}

#[test]
fn option_test_defer_option() {
    #[derive(Copy, Clone, Default, Miniconf)]
    struct S {
        #[miniconf(defer)]
        data: Option<u32>,
    }

    let mut s = S::default();
    assert!(s.data.is_none());

    let mut iterator = S::iter_paths::<10, 128>('/').unwrap();
    assert_eq!(iterator.next(), Some("data".into()));
    assert!(iterator.next().is_none());

    assert!(s.set("data", b"7").is_err());
    s.data = Some(0);
    s.set("data", b"7").unwrap();
    assert_eq!(s.data, Some(7));

    let mut iterator = S::iter_paths::<10, 128>('/').unwrap();
    assert_eq!(iterator.next(), Some("data".into()));
    assert!(iterator.next().is_none());

    assert!(s.set("data", b"null").is_err());
}
