use miniconf::StringSet;
use serde::Deserialize;

#[derive(StringSet, Debug, Deserialize, PartialEq)]
enum Variant {
    #[serde(rename = "A")]
    A,
    #[serde(rename = "B")]
    B,
}

#[derive(StringSet, Debug, Deserialize)]
struct Top {
    a: u32,
    b: u8,
    c: [u8; 3],
    d: Inner,
    f: Variant,
}

#[derive(StringSet, Debug, Deserialize)]
struct Inner {
    e: u32,
}

fn main() {
    let mut t = Top {
        a: 0,
        b: 0,
        c: [0; 3],
        d: Inner { e: 0 },
        f: Variant::A,
    };

    let field = "a".split('/').peekable();


    dbg!(&t);
    t.string_set(field, "5".as_bytes()).unwrap();
    dbg!(&t);

    let field = "c".split('/').peekable();
    t.string_set(field, "[1,2,3]".as_bytes()).unwrap();
    dbg!(&t);

    let field = "d/e".split('/').peekable();
    t.string_set(field, "7".as_bytes()).unwrap();
    dbg!(&t);

    let field = "c/0".split('/').peekable();
    t.string_set(field, "0".as_bytes()).unwrap();
    dbg!(&t);

    let field = "f".split('/').peekable();
    t.string_set(field, "\"B\"".as_bytes()).unwrap();
    dbg!(&t);
}
