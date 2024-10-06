use miniconf::{json, Deserialize, Serialize, Tree, TreeDeserialize, TreeKey, TreeSerialize};

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
        c: Inner,
        #[tree(depth = 1)]
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

    assert_eq!(settings.c, Inner { a: 5 });
    assert_eq!(settings.d, Inner { a: 3 });

    // Check that metadata is correct.
    let metadata = Settings::path_metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length("/"), "/d/a".len());
    assert_eq!(metadata.count, 4);

    assert_eq!(paths::<Settings, 2>(), ["/a", "/b", "/c", "/d/a"]);
}

#[test]
fn empty_struct() {
    #[derive(Tree, Default)]
    struct Settings {}
    assert_eq!(paths::<Settings, 1>(), [""; 0]);
}

#[test]
fn unit_struct() {
    #[derive(Tree, Default)]
    struct Settings;
    assert_eq!(paths::<Settings, 1>(), [""; 0]);
}

#[test]
fn empty_tuple_struct() {
    #[derive(Tree, Default)]
    struct Settings();
    assert_eq!(paths::<Settings, 1>(), [""; 0]);
}

#[test]
fn borrowed() {
    // Can't derive TreeAny
    #[derive(TreeKey, TreeDeserialize, TreeSerialize)]
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
