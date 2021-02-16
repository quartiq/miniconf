use miniconf::{Error, StringSet};
use serde::Deserialize;

#[test]
fn simple_enum() {
    #[derive(StringSet, Debug, Deserialize, PartialEq)]
    enum Variant {
        A,
        B,
    }

    #[derive(StringSet, Debug, Deserialize)]
    struct S {
        v: Variant,
    }

    let mut s = S { v: Variant::A };

    let field = "v".split('/').peekable();

    s.string_set(field, "\"B\"".as_bytes()).unwrap();

    assert_eq!(s.v, Variant::B);
}
