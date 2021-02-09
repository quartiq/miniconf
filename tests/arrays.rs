use miniconf::{Error, StringSet};
use serde::Deserialize;

#[derive(Debug, Default, StringSet, Deserialize)]
struct AdditionalSettings {
    inner: u8,
}

#[derive(Debug, Default, StringSet, Deserialize)]
struct Settings {
    data: u32,
    more: AdditionalSettings,
}

#[test]
fn simple_array() {
    #[derive(StringSet, Default)]
    struct S {
        a: [u8; 3],
    }

    let mut s = S::default();

    let field = "a".split('/').peekable();

    s.string_set(field, "[1,2,3]".as_bytes()).unwrap();

    assert_eq!([1, 2, 3], s.a);
}

#[test]
fn nonexistent_field() {
    #[derive(StringSet, Default)]
    struct S {
        a: [u8; 3],
    }

    let mut s = S::default();

    let field = "a/b/1".split('/').peekable();

    assert!(s.string_set(field, "7".as_bytes()).is_err());
}

#[test]
fn simple_array_indexing() {
    #[derive(StringSet, Default)]
    struct S {
        a: [u8; 3],
    }

    let mut s = S::default();

    let field = "a/1".split('/').peekable();

    s.string_set(field, "7".as_bytes()).unwrap();

    assert_eq!([0, 7, 0], s.a);

    // Ensure that setting an out-of-bounds index generates an error.
    let field = "a/3".split('/').peekable();
    assert_eq!(s.string_set(field, "7".as_bytes()).unwrap_err(), Error::BadIndex);
}

#[test]
fn array_of_structs_indexing() {
    #[derive(StringSet, Default, Clone, Copy, Deserialize, Debug, PartialEq)]
    struct Inner {
        b: u8,
    }

    #[derive(StringSet, Default, PartialEq, Debug)]
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
