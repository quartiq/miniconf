use miniconf::{Miniconf, MiniconfAtomic};
use serde::{Deserialize, Serialize};

#[test]
fn atomic_struct() {
    #[derive(MiniconfAtomic, Default, PartialEq, Debug, Serialize, Deserialize)]
    struct Inner {
        a: u32,
        b: u32,
    }

    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct Settings {
        a: f32,
        b: bool,
        c: Inner,
    }

    let mut settings = Settings::default();

    let field = "c/a".split('/').peekable();

    // Inner settings structure is atomic, so cannot be set.
    assert!(settings.string_set(field, b"4").is_err());

    // Inner settings can be updated atomically.
    let field = "c".split('/').peekable();
    settings.string_set(field, b"{\"a\": 5, \"b\": 3}").unwrap();

    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 5;
        expected.c.b = 3;
        expected
    };

    assert_eq!(settings, expected);
}

#[test]
fn recursive_struct() {
    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct Inner {
        a: u32,
    }

    #[derive(Miniconf, Default, PartialEq, Debug)]
    struct Settings {
        a: f32,
        b: bool,
        c: Inner,
    }

    let mut settings = Settings::default();

    let field = "c/a".split('/').peekable();

    settings.string_set(field, b"3").unwrap();
    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 3;
        expected
    };

    assert_eq!(settings, expected);

    // It is not allowed to set a non-terminal node.
    let field = "c".split('/').peekable();
    assert!(settings.string_set(field, b"{\"a\": 5}").is_err());
}
