use miniconf::{
    json, Deserialize, IntoKeys, Leaf, Metadata, Serialize, Traversal, Tree, TreeAny,
    TreeDeserialize, TreeKey, TreeSerialize,
};

mod common;
use common::*;

#[test]
fn structs() {
    #[derive(Serialize, Deserialize, Tree, Default, PartialEq, Debug)]
    struct Inner {
        a: Leaf<u32>,
    }

    #[derive(Tree, Default, PartialEq, Debug)]
    struct Settings {
        a: Leaf<f32>,
        b: Leaf<bool>,
        c: Leaf<Inner>,
        d: Inner,
    }

    let mut settings = Settings::default();

    // Inner settings structure is atomic, so cannot be set.
    assert!(json::set(&mut settings, "/c/a", b"4").is_err());

    // Inner settings can be updated atomically.
    set_get(&mut settings, "/c", b"{\"a\":5}");

    // Deferred inner settings can be updated individually.
    set_get(&mut settings, "/d/a", b"3");

    // It is not allowed to set a non-terminal node.
    assert!(json::set(&mut settings, "/d", b"{\"a\": 5").is_err());

    assert_eq!(*settings.c, Inner { a: 5.into() });
    assert_eq!(settings.d, Inner { a: 3.into() });

    // Check that metadata is correct.
    let metadata = Settings::traverse_all::<Metadata>().unwrap();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length("/"), "/d/a".len());
    assert_eq!(metadata.count.get(), 4);

    assert_eq!(paths::<Settings, 2>(), ["/a", "/b", "/c", "/d/a"]);
}

#[test]
fn borrowed() {
    // Can't derive TreeAny
    #[derive(TreeKey, TreeDeserialize, TreeSerialize)]
    struct S<'a> {
        a: Leaf<&'a str>,
    }
    let mut s = S { a: "foo".into() };
    set_get(&mut s, "/a", br#""bar""#);
    assert_eq!(s.a, "bar".into());
}

#[test]
fn tuple_struct() {
    #[derive(Tree, Default)]
    struct Settings(Leaf<i32>, Leaf<f32>);

    let mut s = Settings::default();

    set_get(&mut s, "/0", br#"2"#);
    assert_eq!(*s.0, 2);
    set_get(&mut s, "/1", br#"3.0"#);
    assert_eq!(*s.1, 3.0);
    json::set(&mut s, "/2", b"3.0").unwrap_err();
    json::set(&mut s, "/foo", b"3.0").unwrap_err();

    assert_eq!(paths::<Settings, 1>(), ["/0", "/1"]);
}

#[test]
fn deny_access() {
    use core::cell::RefCell;
    #[derive(Tree)]
    struct S<'a> {
        #[tree(deny(deserialize = "no de", mut_any = "no any"))]
        field: Leaf<i32>,
        #[tree(deny(ref_any = "no any", mut_any = "no any"))]
        cell: &'a RefCell<Leaf<i32>>,
    }
    let cell = RefCell::new(2.into());
    let mut s = S {
        field: 1.into(),
        cell: &cell,
    };
    common::set_get(&mut s, "/cell", b"3");
    s.ref_any_by_key([0].into_keys()).unwrap();
    assert!(matches!(
        s.mut_any_by_key([0].into_keys()),
        Err(Traversal::Access(1, "no any"))
    ));
}
