use miniconf::Miniconf;
use serde::{Deserialize, Serialize};

#[test]
fn simple_enum() {
    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    enum Variant {
        A,
        B,
    }

    #[derive(Miniconf, Debug)]
    struct S {
        v: Variant,
    }

    let mut s = S { v: Variant::A };

    let field = "v".split('/').peekable();

    s.set_path(field, "\"B\"".as_bytes()).unwrap();

    assert_eq!(s.v, Variant::B);

    // Test metadata
    let metadata = s.metadata();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_topic_size, "v".len());
}

#[test]
fn invalid_enum() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    enum Variant {
        A,
        B,
    }

    #[derive(Miniconf, Debug)]
    struct S {
        v: Variant,
    }

    let mut s = S { v: Variant::A };

    let field = "v".split('/').peekable();

    assert!(s.set_path(field, "\"C\"".as_bytes()).is_err());
}
