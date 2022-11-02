use miniconf::{Error, Miniconf};
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
}

#[test]
fn simple_array() {
    #[derive(Miniconf, Default)]
    struct S {
        #[miniconf(defer)]
        a: [u8; 3],
    }

    let mut s = S::default();

    // Updating a single field should succeed.
    s.set("a/0", "99".as_bytes()).unwrap();
    assert_eq!(99, s.a[0]);

    // Updating entire array atomically is not supported.
    assert!(s.set("a", "[1,2,3]".as_bytes()).is_err());

    // Invalid index should generate an error.
    assert!(s.set("a/100", "99".as_bytes()).is_err());
}

#[test]
fn nonexistent_field() {
    #[derive(Miniconf, Default)]
    struct S {
        #[miniconf(defer)]
        a: [u8; 3],
    }

    let mut s = S::default();

    assert!(s.set("a/1/b", "7".as_bytes()).is_err());
}

#[test]
fn simple_array_indexing() {
    #[derive(Miniconf, Default)]
    struct S {
        #[miniconf(defer)]
        a: [u8; 3],
    }

    let mut s = S::default();

    s.set("a/1", "7".as_bytes()).unwrap();

    assert_eq!([0, 7, 0], s.a);

    // Ensure that setting an out-of-bounds index generates an error.
    assert_eq!(s.set("a/3", "7".as_bytes()).unwrap_err(), Error::BadIndex);

    // Test metadata
    let metadata = S::metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length, "a/2".len());
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

    s.set("a/1/b", "7".as_bytes()).unwrap();

    let expected = {
        let mut e = S::default();
        e.a[1].b = 7;
        e
    };

    assert_eq!(expected, s);

    // Test metadata
    let metadata = S::metadata();
    assert_eq!(metadata.max_depth, 3);
    assert_eq!(metadata.max_length, "a/2/b".len());
}

#[test]
fn array_of_arrays() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        #[miniconf(defer)]
        data: miniconf::Array<[u32; 2], 2>,
    }

    let mut s = S::default();

    s.set("data/0/0", "7".as_bytes()).unwrap();

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

    s.set("data", "[1, 2]".as_bytes()).unwrap();

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
    let meta = S::metadata();
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "data/0".len());
}

/// Zero-length arrays are not supported
#[test]
#[should_panic]
fn null_array() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        #[miniconf(defer)]
        data: [u32; 0],
    }
    let _meta = S::metadata();
}
