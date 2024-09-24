use miniconf::{
    Deserialize, JsonCoreSlash, Path, Serialize, Tree, TreeDeserialize, TreeKey, TreeSerialize,
};

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
    assert!(settings.set_json("/c/a", b"4").is_err());

    // Inner settings can be updated atomically.
    settings.set_json("/c", b"{\"a\": 5}").unwrap();

    // Deferred inner settings can be updated individually.
    settings.set_json("/d/a", b"3").unwrap();

    // It is not allowed to set a non-terminal node.
    assert!(settings.set_json("/d", b"{\"a\": 5").is_err());

    assert_eq!(settings.c, Inner { a: 5 });
    assert_eq!(settings.d, Inner { a: 3 });

    // Check that metadata is correct.
    let metadata = Settings::metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length("/"), "/d/a".len());
    assert_eq!(metadata.count, 4);

    assert_eq!(
        Settings::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        vec!["/a", "/b", "/c", "/d/a"]
    );
}

#[test]
fn empty_struct() {
    #[derive(Tree, Default)]
    struct Settings {}
    assert!(Settings::nodes::<Path<String, '/'>>()
        .exact_size()
        .next()
        .is_none());
}

#[test]
fn unit_struct() {
    #[derive(Tree, Default)]
    struct Settings;
    assert!(Settings::nodes::<Path<String, '/'>>()
        .exact_size()
        .next()
        .is_none());
}

#[test]
fn empty_tuple_struct() {
    #[derive(Tree, Default)]
    struct Settings();
    assert!(Settings::nodes::<Path<String, '/'>>()
        .exact_size()
        .next()
        .is_none());
}

#[test]
fn borrowed() {
    // Can't derive TreeAny
    #[derive(TreeKey, TreeDeserialize, TreeSerialize)]
    struct S<'a> {
        a: &'a str,
    }
    let mut s = S { a: "foo" };
    s.set_json("/a", br#""bar""#).unwrap();
    assert_eq!(s.a, "bar");
}

#[test]
fn tuple_struct() {
    #[derive(Tree, Default)]
    struct Settings(i32, f32);

    let mut s = Settings::default();

    let mut buf = [0u8; 256];
    let len = s.get_json("/0", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"0");

    s.set_json("/1", b"3.0").unwrap();
    assert_eq!(s.1, 3.0);
    s.set_json("/2", b"3.0").unwrap_err();
    s.set_json("/foo", b"3.0").unwrap_err();

    assert_eq!(
        Settings::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        vec!["/0", "/1"]
    );
}
