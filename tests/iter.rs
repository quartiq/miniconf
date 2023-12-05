#![cfg(feature = "json-core")]

use miniconf::{JsonCoreSlash, Tree, TreeKey};

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    inner: f32,
}

#[derive(Tree, Default, PartialEq, Debug)]
struct Settings {
    a: f32,
    b: i32,
    #[tree()]
    c: Inner,
}

#[test]
fn struct_iter() {
    let mut paths = ["/a", "/b", "/c/inner"].into_iter();
    for (have, expect) in Settings::iter_paths::<String>("/").zip(&mut paths) {
        assert_eq!(have.unwrap(), expect);
    }
    // Ensure that all fields were iterated.
    assert_eq!(paths.next(), None);
}

#[test]
fn struct_iter_indices() {
    let mut paths = [&[0][..], &[1][..], &[2, 0][..]].into_iter();
    for (have, expect) in Settings::iter_indices().zip(&mut paths) {
        assert_eq!(have, expect);
    }
    // Ensure that all fields were iterated.
    assert_eq!(paths.next(), None);
}

#[test]
fn array_iter() {
    #[derive(Tree, Copy, Clone, Default)]
    struct I {
        c: bool,
    }

    #[derive(Tree, Default)]
    struct Settings {
        #[tree()]
        a: [bool; 2],
        #[tree(depth(2))]
        b: [I; 3],
    }

    let mut s = Settings::default();

    for field in Settings::iter_paths::<String>("/") {
        let field = field.unwrap();
        s.set_json(&field, b"true").unwrap();
        let mut buf = [0; 32];
        let len = s.get_json(&field, &mut buf).unwrap();
        assert_eq!(&buf[..len], b"true");
    }

    assert!(s.a.iter().all(|x| *x));
    assert!(s.b.iter().all(|i| i.c));
}
