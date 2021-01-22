use derive_stringset::StringSet;
use serde_json_core;

#[derive(StringSet, Debug)]
struct Top {
    a: u32,
    b: u8,
    c: [u8;3],
}

fn main() {
    let mut t = Top {
        a: 0,
        b: 0,
        c: [0; 3],
    };

    let field = "a".split('/').peekable();
    let value = "5";

    dbg!(&t);
    t.string_set(field, value).unwrap();
    dbg!(&t);

}
