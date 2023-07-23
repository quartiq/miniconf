use miniconf::{Miniconf, MiniconfSpec};
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
    assert!(settings.set("c/a", b"4").is_err());

    // Inner settings can be updated atomically.
    settings.set("c", b"{\"a\": 5, \"b\": 3}").unwrap();

    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 5;
        expected.c.b = 3;
        expected
    };

    assert_eq!(settings, expected);

    // Check that metadata is correct.
    let metadata = Settings::metadata();
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "c".len());
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

    settings.set("c/a", b"3").unwrap();
    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 3;
        expected
    };

    assert_eq!(settings, expected);

    // It is not allowed to set a non-terminal node.
    assert!(settings.set("c", b"{\"a\": 5}").is_err());

    // Check that metadata is correct.
    let metadata = Settings::metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length, "c/a".len());
    assert_eq!(metadata.count, 3);
}

#[test]
fn struct_with_string() {
    #[derive(Miniconf, Default)]
    struct Settings {
        string: heapless::String<10>,
    }

    let mut s = Settings::default();

    let mut buf = [0u8; 256];
    let len = s.get("string", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"\"\"");

    s.set("string", br#""test""#).unwrap();
    assert_eq!(s.string, "test");
}

#[test]
fn empty_struct() {
    #[derive(Miniconf, Default)]
    struct Settings {}
    assert!(Settings::iter_paths::<1, 0>().unwrap().next().is_none());
}
