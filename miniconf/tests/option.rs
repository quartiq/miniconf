use miniconf::{json, Error, Leaf, Traversal, Tree};

mod common;
use common::*;

#[derive(PartialEq, Debug, Clone, Default, Tree)]
struct Inner {
    data: Leaf<u32>,
}

#[derive(Debug, Clone, Default, Tree)]
struct Settings {
    value: Option<Inner>,
}

#[test]
fn just_option() {
    assert_eq!(paths::<Option<Leaf<u32>>, 1>(), [""]);
}

#[test]
fn option_get_set_none() {
    let mut settings = Settings::default();
    let mut data = [0; 100];

    // Check that if the option is None, the value cannot be get or set.
    settings.value.take();
    assert_eq!(
        json::get(&settings, "/value_foo", &mut data),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        json::get(&settings, "/value", &mut data),
        Err(Traversal::Absent(1).into())
    );
    // The Absent field indicates at which depth the variant was absent
    assert_eq!(
        json::set(&mut settings, "/value/data", b"5"),
        Err(Traversal::Absent(1).into())
    );
}

#[test]
fn option_get_set_some() {
    let mut settings = Settings::default();

    // Check that if the option is Some, the value can be get or set.
    settings.value.replace(Inner { data: 5.into() });

    set_get(&mut settings, "/value/data", b"7");
    assert_eq!(*settings.value.unwrap().data, 7);
}

#[test]
fn option_iterate_some_none() {
    assert_eq!(paths::<Settings, 3>(), ["/value/data"]);
}

#[test]
fn option_test_normal_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        data: Leaf<Option<u32>>,
    }
    assert_eq!(paths::<S, 1>(), ["/data"]);

    let mut s = S::default();
    assert!(s.data.is_none());

    set_get(&mut s, "/data", b"7");
    assert_eq!(*s.data, Some(7));

    set_get(&mut s, "/data", b"null");
    assert!(s.data.is_none());
}

#[test]
fn option_test_defer_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        data: Option<Leaf<u32>>,
    }
    assert_eq!(paths::<S, 1>(), ["/data"]);

    let mut s = S::default();
    assert!(s.data.is_none());

    assert!(json::set(&mut s, "/data", b"7").is_err());
    s.data = Some(0.into());
    set_get(&mut s, "/data", b"7");
    assert_eq!(s.data, Some(7.into()));

    assert!(json::set(&mut s, "/data", b"null").is_err());
}

#[test]
fn option_absent() {
    #[derive(Copy, Clone, Default, Tree)]
    struct I(Leaf<()>);

    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        d: Option<Leaf<u32>>,
        dm: Option<I>,
    }

    let mut s = S::default();
    assert_eq!(
        json::set(&mut s, "/d", b"7"),
        Err(Traversal::Absent(1).into())
    );
    // Check precedence
    assert_eq!(
        json::set(&mut s, "/d", b""),
        Err(Traversal::Absent(1).into())
    );
    assert_eq!(
        json::set(&mut s, "/d/foo", b"7"),
        Err(Traversal::Absent(1).into())
    );
    assert_eq!(
        json::set(&mut s, "", b"7"),
        Err(Traversal::TooShort(0).into())
    );
    s.d = Some(3.into());
    assert_eq!(json::set(&mut s, "/d", b"7"), Ok(1));
    assert_eq!(
        json::set(&mut s, "/d/foo", b"7"),
        Err(Traversal::TooLong(1).into())
    );
    assert!(matches!(
        json::set(&mut s, "/d", b""),
        Err(Error::Inner(1, _))
    ));
    assert_eq!(json::set(&mut s, "/d", b"7 "), Ok(2));
    assert_eq!(json::set(&mut s, "/d", b" 7"), Ok(2));
    assert!(matches!(
        json::set(&mut s, "/d", b"7i"),
        Err(Error::Finalization(_))
    ));
}

#[test]
fn array_option() {
    // This tests that no invalid bounds are inferred for Options and Options in arrays.
    #[allow(dead_code)]
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        a: Option<Leaf<u32>>,
        b: [Leaf<Option<u32>>; 1],
        c: [Option<Leaf<u32>>; 1],
    }
}
