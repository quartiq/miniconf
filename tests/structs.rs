use miniconf::Miniconf;
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

    let field = "c/a".split('/').peekable();

    // Inner settings structure is atomic, so cannot be set.
    assert!(settings.set_path(field, b"4").is_err());

    // Inner settings can be updated atomically.
    let field = "c".split('/').peekable();
    settings.set_path(field, b"{\"a\": 5, \"b\": 3}").unwrap();

    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 5;
        expected.c.b = 3;
        expected
    };

    assert_eq!(settings, expected);

    // Check that metadata is correct.
    let metadata = settings.metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length, "c".len());
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

    let field = "c/a".split('/').peekable();

    settings.set_path(field, b"3").unwrap();
    let expected = {
        let mut expected = Settings::default();
        expected.c.a = 3;
        expected
    };

    assert_eq!(settings, expected);

    // It is not allowed to set a non-terminal node.
    let field = "c".split('/').peekable();
    assert!(settings.set_path(field, b"{\"a\": 5}").is_err());

    // Check that metadata is correct.
    let metadata = settings.metadata();
    assert_eq!(metadata.max_depth, 3);
    assert_eq!(metadata.max_length, "c/a".len());
}

#[test]
fn struct_with_string() {
    #[derive(Miniconf, Default)]
    struct Settings {
        string: heapless::String<10>,
    }

    let mut s = Settings::default();

    let field = "string".split('/').peekable();
    let mut buf = [0u8; 256];
    let len = s.get_path(field, &mut buf).unwrap();
    assert_eq!(&buf[..len], b"\"\"");

    let field = "string".split('/').peekable();
    s.set_path(field, br#""test""#).unwrap();
    assert_eq!(s.string, "test");
}
