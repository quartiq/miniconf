#![cfg(feature = "json-core")]

use miniconf::{JsonCoreSlash, Miniconf};
use serde::{Deserialize, Serialize};

#[test]
fn atomic_struct() {
    #[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
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

    // Inner settings structure is atomic, so cannot be set.
    assert!(settings.set_json("/c/a", b"4").is_err());

    // Inner settings can be updated atomically.
    settings.set_json("/c", b"{\"a\": 5, \"b\": 3}").unwrap();

    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 5;
        expected.c.b = 3;
        expected
    };

    assert_eq!(settings, expected);

    // Check that metadata is correct.
    let metadata = Settings::metadata().separator("/");
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "/c".len());
    assert_eq!(metadata.count, 3);
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
        #[miniconf(defer)]
        c: Inner,
    }

    let mut settings = Settings::default();

    settings.set_json("/c/a", b"3").unwrap();
    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 3;
        expected
    };

    assert_eq!(settings, expected);

    // It is not allowed to set a non-terminal node.
    assert!(settings.set_json("/c", b"{\"a\": 5}").is_err());

    // Check that metadata is correct.
    let metadata = Settings::metadata().separator("/");
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length, "/c/a".len());
    assert_eq!(metadata.count, 3);
}

#[test]
fn empty_struct() {
    #[derive(Miniconf, Default)]
    struct Settings {}
    assert!(Settings::iter_paths::<1, String>("/")
        .unwrap()
        .next()
        .is_none());
}
