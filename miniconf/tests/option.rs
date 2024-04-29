#![cfg(all(feature = "json-core", feature = "derive"))]

use miniconf::{Error, JsonCoreSlash, Traversal, Tree, TreeKey};

#[derive(PartialEq, Debug, Clone, Default, Tree)]
struct Inner {
    data: u32,
}

#[derive(Debug, Clone, Default, Tree)]
struct Settings {
    #[tree(depth = 2)]
    value: Option<Inner>,
}

#[test]
fn just_option() {
    let mut it = Option::<u32>::iter_paths::<String>("/").count();
    assert_eq!(it.next(), Some(Ok("".into())));
    assert_eq!(it.next(), None);
}

#[test]
fn option_get_set_none() {
    let mut settings = Settings::default();
    let mut data = [0; 100];

    // Check that if the option is None, the value cannot be get or set.
    settings.value.take();
    assert_eq!(
        settings.get_json("/value_foo", &mut data),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        settings.get_json("/value", &mut data),
        Err(Traversal::Absent(1).into())
    );
    assert_eq!(
        settings.set_json("/value/data", b"5"),
        Err(Traversal::Absent(1).into())
    );
}

#[test]
fn option_get_set_some() {
    let mut settings = Settings::default();
    let mut data = [0; 10];

    // Check that if the option is Some, the value can be get or set.
    settings.value.replace(Inner { data: 5 });

    let len = settings.get_json("/value/data", &mut data).unwrap();
    assert_eq!(&data[..len], b"5");

    settings.set_json("/value/data", b"7").unwrap();
    assert_eq!(settings.value.unwrap().data, 7);
}

#[test]
fn option_iterate_some_none() {
    let mut settings = Settings::default();

    // When the value is None, it will still be iterated over as a topic but may not exist at runtime.
    settings.value.take();
    let mut iterator = Settings::iter_paths::<String>("/").count();
    assert_eq!(iterator.next(), Some(Ok("/value/data".into())));
    assert!(iterator.next().is_none());

    // When the value is Some, it should be iterated over.
    settings.value.replace(Inner { data: 5 });
    let mut iterator = Settings::iter_paths::<String>("/").count();
    assert_eq!(iterator.next(), Some(Ok("/value/data".into())));
    assert_eq!(iterator.next(), None);
}

#[test]
fn option_test_normal_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        data: Option<u32>,
    }

    let mut s = S::default();
    assert!(s.data.is_none());

    let mut iterator = S::iter_paths::<String>("/").count();
    assert_eq!(iterator.next(), Some(Ok("/data".into())));
    assert!(iterator.next().is_none());

    s.set_json("/data", b"7").unwrap();
    assert_eq!(s.data, Some(7));

    let mut iterator = S::iter_paths::<String>("/").count();
    assert_eq!(iterator.next(), Some(Ok("/data".into())));
    assert!(iterator.next().is_none());

    s.set_json("/data", b"null").unwrap();
    assert!(s.data.is_none());
}

#[test]
fn option_test_defer_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        #[tree(depth = 1)]
        data: Option<u32>,
    }

    let mut s = S::default();
    assert!(s.data.is_none());

    let mut iterator = S::iter_paths::<String>("/").count();
    assert_eq!(iterator.next(), Some(Ok("/data".into())));
    assert!(iterator.next().is_none());

    assert!(s.set_json("/data", b"7").is_err());
    s.data = Some(0);
    s.set_json("/data", b"7").unwrap();
    assert_eq!(s.data, Some(7));

    let mut iterator = S::iter_paths::<String>("/").count();
    assert_eq!(iterator.next(), Some(Ok("/data".into())));
    assert!(iterator.next().is_none());

    assert!(s.set_json("/data", b"null").is_err());
}

#[test]
fn option_absent() {
    #[derive(Copy, Clone, Default, Tree)]
    struct I {}

    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        #[tree(depth = 1)]
        d: Option<u32>,
        #[tree(depth = 2)]
        dm: Option<I>,
    }

    let mut s = S::default();
    assert_eq!(s.set_json("/d", b"7"), Err(Traversal::Absent(1).into()));
    // Check precedence
    assert_eq!(s.set_json("/d", b""), Err(Traversal::Absent(1).into()));
    assert_eq!(
        s.set_json("/d/foo", b"7"),
        Err(Traversal::TooLong(1).into())
    );
    assert_eq!(s.set_json("", b"7"), Err(Traversal::TooShort(0).into()));
    s.d = Some(3);
    assert_eq!(s.set_json("/d", b"7"), Ok(1));
    assert_eq!(
        s.set_json("/d/foo", b"7"),
        Err(Traversal::TooLong(1).into())
    );
    assert!(matches!(s.set_json("/d", b""), Err(Error::Inner(1, _))));
    assert_eq!(s.set_json("/d", b"7 "), Ok(2));
    assert_eq!(s.set_json("/d", b" 7"), Ok(2));
    assert!(matches!(
        s.set_json("/d", b"7i"),
        Err(Error::Finalization(_))
    ));
}

#[test]
fn array_option() {
    // This tests that no invalid bounds are inferred for Options and Options in arrays.
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        #[tree(depth = 1)]
        a: Option<u32>,
        #[tree(depth = 1)]
        b: [Option<u32>; 1],
        #[tree(depth = 2)]
        c: [Option<u32>; 1],
    }
}
