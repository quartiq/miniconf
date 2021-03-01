use miniconf::{Error, Miniconf};
use serde::Deserialize;

#[derive(Debug, Default, Miniconf, Deserialize)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Debug, Default, Miniconf, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

#[test]
fn simple_array() {
    #[derive(Miniconf, Default)]
    struct S {
        a: [u8; 3],
    }

    let mut s = S::default();

    // Updating a single field should succeed.
    let field = "a/0".split('/').peekable();
    s.string_set(field, "99".as_bytes()).unwrap();
    assert_eq!(99, s.a[0]);

    // Updating entire array atomically is not supported.
    let field = "a".split('/').peekable();
    assert!(s.string_set(field, "[1,2,3]".as_bytes()).is_err());

    // Invalid index should generate an error.
    let field = "a/100".split('/').peekable();
    assert!(s.string_set(field, "99".as_bytes()).is_err());
}

#[test]
fn nonexistent_field() {
    #[derive(Miniconf, Default)]
    struct S {
        a: [u8; 3],
    }

    let mut s = S::default();

    let field = "a/b/1".split('/').peekable();

    assert!(s.string_set(field, "7".as_bytes()).is_err());
}

#[test]
fn simple_array_indexing() {
    #[derive(Miniconf, Default)]
    struct S {
        a: [u8; 3],
    }

    let mut s = S::default();

    let field = "a/1".split('/').peekable();

    s.string_set(field, "7".as_bytes()).unwrap();

    assert_eq!([0, 7, 0], s.a);

    // Ensure that setting an out-of-bounds index generates an error.
    let field = "a/3".split('/').peekable();
    assert_eq!(
        s.string_set(field, "7".as_bytes()).unwrap_err(),
        Error::BadIndex
    );
}

#[test]
fn array_of_structs_indexing() {
    #[derive(Miniconf, Default, Clone, Copy, Deserialize, Debug, PartialEq)]
    struct Inner {
        b: u8,
    }

    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct S {
        a: [Inner; 3],
    }

    let mut s = S::default();

    let field = "a/1/b".split('/').peekable();

    s.string_set(field, "7".as_bytes()).unwrap();

    let expected = {
        let mut e = S::default();
        e.a[1].b = 7;
        e
    };

    assert_eq!(expected, s);
}
