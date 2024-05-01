use miniconf::{Deserialize, JsonCoreSlash, Serialize, Traversal, Tree, TreeKey};

#[derive(Debug, Copy, Clone, Default, Tree, Deserialize, Serialize)]
struct Inner {
    c: u8,
}

#[derive(Debug, Default, Tree)]
struct Settings {
    a: [u8; 2],
    #[tree(depth = 1)]
    d: [u8; 2],
    #[tree(depth = 1)]
    dm: [Inner; 2],
    #[tree(depth = 2)]
    am: [Inner; 2],
    #[tree(depth = 3)]
    aam: [[Inner; 2]; 2],
}

#[test]
fn atomic() {
    let mut s = Settings::default();
    s.set_json("/a", b"[1,2]").unwrap();
    assert_eq!(s.a, [1, 2]);
}

#[test]
fn defer() {
    let mut s = Settings::default();
    s.set_json("/d/1", b"99").unwrap();
    assert_eq!(s.d[1], 99);
}

#[test]
fn defer_miniconf() {
    let mut s = Settings::default();
    s.set_json("/am/0/c", b"1").unwrap();
    assert_eq!(s.am[0].c, 1);
    s.set_json("/aam/0/0/c", b"3").unwrap();
    assert_eq!(s.aam[0][0].c, 3);
}

#[test]
fn too_short() {
    let mut s = Settings::default();
    assert_eq!(
        s.set_json("/d", b"[1,2]"),
        Err(Traversal::TooShort(1).into())
    );
    // Check precedence over `Inner`.
    assert_eq!(
        s.set_json("/d", b"[1,2,3]"),
        Err(Traversal::TooShort(1).into())
    );
}

#[test]
fn too_long() {
    assert_eq!(
        Settings::default().set_json("/a/1", b"7"),
        Err(Traversal::TooLong(1).into())
    );
    assert_eq!(
        Settings::default().set_json("/d/0/b", b"7"),
        Err(Traversal::TooLong(2).into())
    );
    assert_eq!(
        Settings::default().set_json("/dm/0/c", b"7"),
        Err(Traversal::TooLong(2).into())
    );
    assert_eq!(
        Settings::default().set_json("/dm/0/d", b"7"),
        Err(Traversal::TooLong(2).into())
    );
}

#[test]
fn not_found() {
    assert_eq!(
        Settings::default().set_json("/d/3", b"7"),
        Err(Traversal::NotFound(2).into())
    );
    assert_eq!(
        Settings::default().set_json("/b", b"7"),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        Settings::default().set_json("/aam/0/0/d", b"7"),
        Err(Traversal::NotFound(4).into())
    );
}

#[test]
fn metadata() {
    let metadata = Settings::metadata();
    assert_eq!(metadata.max_depth, 4);
    assert_eq!(metadata.max_length("/"), "/aam/0/0/c".len());
    assert_eq!(metadata.count, 11);
}

#[test]
fn empty() {
    assert!(<[u32; 0]>::iter_paths::<String>("")
        .count()
        .next()
        .is_none());

    #[derive(Tree, Serialize, Deserialize)]
    struct S {}

    assert!(<[S; 0] as TreeKey>::iter_paths::<String>("")
        .count()
        .next()
        .is_none());
    assert!(<[[S; 0]; 0] as TreeKey>::iter_paths::<String>("")
        .count()
        .next()
        .is_none());

    #[derive(Tree)]
    struct Q {
        #[tree(depth = 2)]
        a: [S; 0],
        #[tree(depth = 1)]
        b: [S; 0],
    }
    assert!(Q::iter_paths::<String>("").count().next().is_none());
}
