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
    #[miniconf(defer)]
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
fn array_iter() {
    #[derive(Tree, Copy, Clone, Default)]
    struct I {
        c: bool,
    }

    #[derive(Tree, Default)]
    struct Settings {
        #[miniconf(defer)]
        a: [bool; 2],
        #[miniconf(defer(2))]
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
