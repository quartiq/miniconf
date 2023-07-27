#![cfg(feature = "json-core")]

use miniconf::{Error, Miniconf, SerDe};
use serde::Deserialize;

#[derive(Debug, Default, Miniconf, Deserialize)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
    #[miniconf(defer)]
    more: AdditionalSettings,
    #[miniconf(defer)]
    a: [u8; 3],
}

#[test]
fn simple_array() {
    let mut s = Settings::default();

    // Updating a single field should succeed.
    s.set("/a/0", "99".as_bytes()).unwrap();
    assert_eq!(99, s.a[0]);

    // Updating entire array atomically is not supported.
    assert!(s.set("/a", "[1,2,3]".as_bytes()).is_err());

    // Invalid index should generate an error.
    assert!(s.set("/a/100", "99".as_bytes()).is_err());
}

#[test]
fn nonexistent_field() {
    assert_eq!(
        Settings::default().set("/a/1/b", b"7"),
        Err(Error::TooLong(1))
    );
}

#[test]
fn simple_array_indexing() {
    #[derive(Miniconf, Default)]
    struct S {
        #[miniconf(defer)]
        a: [u8; 3],
    }

    let mut s = S::default();

    s.set("/a/1", b"7").unwrap();

    assert_eq!([0, 7, 0], s.a);

    // Ensure that setting an out-of-bounds index generates an error.
    assert_eq!(s.set("/a/3", b"7"), Err(Error::NotFound(1)));

    // Test metadata
    let metadata = S::metadata(1);
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length, "/a/2".len());
    assert_eq!(metadata.count, 3);
}

#[test]
fn array_iter() {
    #[derive(Miniconf, Default, Clone, Copy, Debug, PartialEq)]
    struct Inner {
        b: u8,
    }

    #[derive(Miniconf, Default)]
    struct S {
        #[miniconf(defer)]
        a: miniconf::Array<miniconf::Array<Inner, 2>, 2>,
    }

    let mut s = S::default();

    for _i in s.a.into_iter().flatten() {}

    for _i in s.a.iter().flatten() {}

    for _i in s.a.iter_mut().flatten() {}
}

#[test]
fn array_of_structs_indexing() {
    #[derive(Miniconf, Default, Clone, Copy, Debug, PartialEq)]
    struct Inner {
        b: u8,
    }

    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        #[miniconf(defer)]
        a: miniconf::Array<Inner, 3>,
    }

    let mut s = S::default();

    s.set("/a/1/b", "7".as_bytes()).unwrap();

    let expected = {
        let mut e = S::default();
        e.a[1].b = 7;
        e
    };

    assert_eq!(expected, s);

    // Test metadata
    let metadata = S::metadata(1);
    assert_eq!(metadata.max_depth, 3);
    assert_eq!(metadata.max_length, "/a/2/b".len());
    assert_eq!(metadata.count, 3);
}

#[test]
fn array_of_arrays() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        #[miniconf(defer)]
        data: miniconf::Array<[u32; 2], 2>,
    }

    let mut s = S::default();

    s.set("/data/0/0", "7".as_bytes()).unwrap();

    let expected = {
        let mut e = S::default();
        e.data[0][0] = 7;
        e
    };

    assert_eq!(expected, s);
}

#[test]
fn atomic_array() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        data: [u32; 2],
    }

    let mut s = S::default();

    s.set("/data", "[1, 2]".as_bytes()).unwrap();

    let expected = {
        let mut e = S::default();
        e.data[0] = 1;
        e.data[1] = 2;
        e
    };

    assert_eq!(expected, s);
}

#[test]
fn short_array() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        #[miniconf(defer)]
        data: [u32; 1],
    }

    // Test metadata
    let meta = S::metadata(1);
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "/data/0".len());
    assert_eq!(meta.count, 1);
}

#[test]
fn null_array() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        #[miniconf(defer)]
        data: [u32; 0],
    }
    assert!(S::iter_paths::<2, String>().unwrap().next().is_none());
}

/*
#[test]
fn null_miniconf_array() {
    #[derive(Miniconf)]
    struct I {
    }
    #[derive(Miniconf)]
    struct S {
        #[miniconf(defer)]
        data: miniconf::Array<I, 1>,
    }
    //assert!(S::iter_paths::<3, String>().unwrap().next().is_none());
}
 */
