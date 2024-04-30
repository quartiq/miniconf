#![cfg(all(feature = "json-core", feature = "derive"))]

use miniconf::{JsonCoreSlash, PathIter, Tree, TreeKey};

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    inner: bool,
}

#[derive(Tree, Default, PartialEq, Debug)]
struct Settings {
    #[tree(depth = 1)]
    b: [bool; 2],
    #[tree(depth = 1)]
    c: Inner,
    #[tree(depth = 2)]
    d: [Inner; 1],
    a: bool,
}

#[test]
fn struct_iter() {
    assert_eq!(
        Settings::iter_paths::<String>("/")
            .count()
            .map(|p| p.unwrap())
            .collect::<Vec<_>>(),
        ["/b/0", "/b/1", "/c/inner", "/d/0/inner", "/a"]
    );
}

#[test]
fn struct_iter_indices() {
    let mut paths = [
        ([0, 0, 0], 2),
        ([0, 1, 0], 2),
        ([1, 0, 0], 2),
        ([2, 0, 0], 3),
        ([3, 0, 0], 1),
    ]
    .into_iter();
    for (have, expect) in Settings::iter_indices().count().zip(&mut paths) {
        assert_eq!(have, expect);
    }
    // Ensure that all fields were iterated.
    assert_eq!(paths.next(), None);
}

#[test]
fn array_iter() {
    let mut s = Settings::default();

    for field in Settings::iter_paths::<String>("/").count() {
        let field = field.unwrap();
        s.set_json(&field, b"true").unwrap();
        let mut buf = [0; 32];
        let len = s.get_json(&field, &mut buf).unwrap();
        assert_eq!(&buf[..len], b"true");
    }

    assert!(s.a);
    assert!(s.b.iter().all(|x| *x));
    assert!(s.c.inner);
    assert!(s.d.iter().all(|i| i.inner));
}

#[test]
fn short_iter() {
    assert_eq!(
        PathIter::<Settings, 3, String, 1>::new("/")
            .map(|p| p.unwrap())
            .collect::<Vec<_>>(),
        ["/a"]
    );

    assert!(PathIter::<Settings, 3, String, 0>::new("/")
        .next()
        .is_none());
}

#[test]
#[should_panic]
fn panic_short_iter() {
    PathIter::<Settings, 3, String, 1>::new("/").count();
}

#[test]
#[should_panic]
fn panic_started_iter() {
    let mut it = Settings::iter_indices();
    it.next();
    it.count();
}
