use core::fmt::Debug;
use core::ops::AddAssign;
use crosstrait::{register, Cast};

use miniconf::{JsonPath, TreeAny, TreeKey};

register! { u8 => dyn Debug }
register! { i32 => dyn AddAssign<i32> + Sync }
register! { i32 => dyn erased_serde::Serialize }

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

fn main() {
    let mut s = Settings::default();
    s.i[1].a = 9;

    let key: JsonPath = ".i[1].a".into();
    let a: &dyn Debug = s.ref_any_by_key(key).unwrap().cast().unwrap();
    println!("{a:?}");

    let key: JsonPath = ".v".into();
    let v: &mut (dyn AddAssign<i32> + Sync) = s.mut_any_by_key(key).unwrap().cast().unwrap();
    *v += 3;
    assert_eq!(s.v, 3);
}
