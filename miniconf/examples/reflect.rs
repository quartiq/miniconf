use erased_serde::{Error, Serialize, Serializer};
use intertrait::cast::CastRef;

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

// Target trait registration hapens at impl time: we need a "newtrait"...
trait Ser: Serialize {
    fn ser(&self, ser: &mut dyn Serializer) -> Result<(), Error>;
}

macro_rules! ser {
    ($($ty:ty)+) => {$(
        #[intertrait::cast_to]
        impl Ser for $ty {
            fn ser(&self, ser: &mut dyn Serializer) -> Result<(), Error> {
                self.erased_serialize(ser)
            }
        }
    )+}
}
ser!(bool usize u8 u16 u32 u128 isize i8 i16 i32 i64 i128); // ...

fn main() {
    let mut s = Settings::default();

    s.i[1].a = 9;
    let key: JsonPath = ".i[1].a".into();

    let a: &dyn Ser = s.ref_any_by_key(key).unwrap().cast().unwrap();
    let mut buf = [0; 10];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    a.ser(&mut <dyn Serializer>::erase(&mut ser)).unwrap();
    let len = ser.end();

    assert_eq!(&buf[..len], b"9");
}
