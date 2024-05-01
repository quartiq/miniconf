use erased_serde::{Serialize, Serializer};
use intertrait::{cast::CastRef, castable_to};

use miniconf::{JsonPath, TreeAny, TreeKey};

#[derive(TreeKey, TreeAny, Default)]
struct Inner {
    a: u8,
}

#[derive(TreeKey, TreeAny, Default)]
struct Settings {
    v: i32,
    #[tree(depth = 2)]
    i: [Inner; 2],
}

castable_to! {u8 => Serialize}
castable_to! {i32 => Serialize}

fn main() {
    let mut s = Settings::default();

    s.i[1].a = 9;
    let key: JsonPath = ".i[1].a".into();

    let a: &dyn Serialize = s.ref_any_by_key(key).unwrap().cast().unwrap();
    let mut buf = [0; 10];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    a.erased_serialize(&mut <dyn Serializer>::erase(&mut ser))
        .unwrap();
    let len = ser.end();

    assert_eq!(&buf[..len], b"9");
}
