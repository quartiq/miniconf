use miniconf::{
    json, Deserialize, IntoKeys, Leaf, Serialize, Shape, Tree, TreeAny, TreeDeserialize,
    TreeSchema, TreeSerialize, ValueError,
};

mod common;
use common::*;

#[test]
fn structs() {
    #[derive(Serialize, Deserialize, Tree, Default, PartialEq, Debug)]
    struct Inner {
        a: u32,
    }

    #[derive(Tree, Default, PartialEq, Debug)]
    struct Settings {
        a: f32,
        b: bool,
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

    assert_eq!(*settings.c, Inner { a: 5 });
    assert_eq!(settings.d, Inner { a: 3 });

    // Check that metadata is correct.
    const SHAPE: Shape = Settings::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 2);
    assert_eq!(SHAPE.max_length("/"), "/d/a".len());
    assert_eq!(SHAPE.count.get(), 4);

    assert_eq!(paths::<Settings, 2>(), ["/a", "/b", "/c", "/d/a"]);
}

#[test]
fn borrowed() {
    // Can't derive TreeAny
    #[derive(TreeSchema, TreeDeserialize, TreeSerialize)]
    struct S<'a> {
        a: &'a str,
    }
    let mut s = S { a: "foo" };
    set_get(&mut s, "/a", br#""bar""#);
    assert_eq!(s.a, "bar");
}

#[test]
fn tuple_struct() {
    #[derive(Tree, Default)]
    struct Settings(i32, f32);

    let mut s = Settings::default();

    set_get(&mut s, "/0", br#"2"#);
    assert_eq!(s.0, 2);
    set_get(&mut s, "/1", br#"3.0"#);
    assert_eq!(s.1, 3.0);
    json::set(&mut s, "/2", b"3.0").unwrap_err();
    json::set(&mut s, "/foo", b"3.0").unwrap_err();

    assert_eq!(paths::<Settings, 1>(), ["/0", "/1"]);
}

#[test]
fn deny_access() {
    use core::cell::RefCell;
    #[derive(Tree)]
    struct S<'a> {
        #[tree(with=deny_write)]
        field: i32,
        #[tree(with=deny_ref)]
        cell: &'a RefCell<i32>,
    }
    let cell = RefCell::new(2);
    let mut s = S {
        field: 1,
        cell: &cell,
    };
    mod deny_write {
        pub use miniconf::{
            deny::{deserialize_by_key, mut_any_by_key},
            leaf::{probe_by_key, ref_any_by_key, serialize_by_key, Type},
        };
    }
    mod deny_ref {
        pub use miniconf::{
            deny::{mut_any_by_key, ref_any_by_key},
            passthrough::{deserialize_by_key, probe_by_key, serialize_by_key, Type},
        };
    }

    common::set_get(&mut s, "/cell", b"3");
    s.ref_any_by_key([0].into_keys()).unwrap();
    assert!(matches!(
        s.mut_any_by_key([0].into_keys()),
        Err(ValueError::Access("Denied"))
    ));
}
