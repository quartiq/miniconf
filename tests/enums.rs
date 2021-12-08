use miniconf::Miniconf;
use serde::{Deserialize, Serialize};

#[test]
fn simple_enum() {
    #[derive(Miniconf, Debug, Deserialize, Serialize, PartialEq)]
    enum Variant {
        A,
        B,
    }

    #[derive(Miniconf, Debug, Deserialize, Serialize)]
    struct S {
        v: Variant,
    }

    let mut s = S { v: Variant::A };

    let field = "v".split('/').peekable();

    s.string_set(field, "\"B\"".as_bytes()).unwrap();

    assert_eq!(s.v, Variant::B);
}

#[test]
fn invalid_enum() {
    #[derive(Miniconf, Debug, Serialize, Deserialize, PartialEq)]
    enum Variant {
        A,
        B,
    }

    #[derive(Miniconf, Debug, Deserialize)]
    struct S {
        v: Variant,
    }

    let mut s = S { v: Variant::A };

    let field = "v".split('/').peekable();

    assert!(s.string_set(field, "\"C\"".as_bytes()).is_err());
}
