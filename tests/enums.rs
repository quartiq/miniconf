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

    s.set("v", "\"B\"".as_bytes()).unwrap();

    assert_eq!(s.v, Variant::B);

    // Test metadata
    let metadata = S::metadata();
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "v".len());
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

    assert!(s.set("v", "\"C\"".as_bytes()).is_err());
}
