#![cfg(feature = "json-core")]

use miniconf::{JsonCoreSlash, Miniconf};

#[derive(Miniconf, Default, PartialEq, Debug)]
struct Inner {
    inner: f32,
}

#[derive(Miniconf, Default, PartialEq, Debug)]
struct Settings {
    a: f32,
    b: i32,
    #[miniconf(defer)]
    c: Inner,
}

#[test]
fn slice_short() {
    let meta = Settings::metadata().separator("/");
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "/c/inner".len());
    assert_eq!(meta.count, 3);

    // Ensure that we can't iterate if we make a state vector that is too small.
    assert_eq!(
        Settings::iter_paths::<1, String>(""),
        Err(miniconf::SliceShort)
    );
}

#[test]
fn struct_iter() {
    let mut paths = ["/a", "/b", "/c/inner"].into_iter();
    for (have, expect) in Settings::iter_paths::<32, String>("/")
        .unwrap()
        .zip(&mut paths)
    {
        assert_eq!(have.unwrap(), expect);
    }
    // Ensure that all fields were iterated.
    assert_eq!(paths.next(), None);
}

#[test]
fn array_iter() {
    #[derive(Miniconf, Copy, Clone, Default)]
    struct I {
        c: bool,
    }

    #[derive(Miniconf, Default)]
    struct Settings {
        #[miniconf(defer)]
        a: [bool; 2],
        #[miniconf(defer)]
        b: miniconf::Array<I, 3>,
    }

    let mut s = Settings::default();

    for field in Settings::iter_paths::<4, String>("/").unwrap() {
        let field = field.unwrap();
        s.set_json(&field, b"true").unwrap();
        let mut buf = [0; 32];
        let len = s.get_json(&field, &mut buf).unwrap();
        assert_eq!(&buf[..len], b"true");
    }

    assert!(s.a.iter().all(|x| *x));
    assert!(s.b.iter().all(|i| i.c));
}
