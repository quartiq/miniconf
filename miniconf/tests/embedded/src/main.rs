#![no_main]
#![no_std]
#![cfg_attr(feature = "used_linker", feature(used_with_arg))]
extern crate panic_semihosting;

use core::any::Any;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};

use crosstrait::{register, Cast};
use miniconf::{self, json, IntoKeys, JsonPath, Leaf, Node, Packed, Path, Tree, TreeAny, TreeKey};

use core::ops::{AddAssign, SubAssign};
register! { i32 => dyn AddAssign<i32> }
register! { u32 => dyn SubAssign<u32> }

#[derive(Default, Tree)]
struct Inner {
    val: Leaf<i32>,
}

#[derive(Default, Tree)]
struct Settings {
    a: [Leaf<i32>; 2],
    i: [Inner; 3],
    b: Option<Leaf<i32>>,
}

#[entry]
fn main() -> ! {
    assert_eq!(crosstrait::REGISTRY_KV.len(), 2);
    hprintln!(
        "registry RAM: {}",
        core::mem::size_of_val(&crosstrait::REGISTRY)
    );

    let mut a = 3i32;
    let any: &mut dyn Any = &mut a;

    let val: &mut dyn AddAssign<i32> = any.cast().unwrap();
    *val += 5;
    assert_eq!(a, 3 + 5);

    let mut s = Settings::default();

    let path = Path::<_, '/'>::from("/i/1/val");
    json::set_by_key(&mut s, &path, b"3").unwrap();

    let (packed, node) = Settings::transcode::<Packed, _>(&path).unwrap();
    assert_eq!(packed.into_lsb().get(), 0b1_01_01_0);
    assert_eq!(node, Node::leaf(3));

    let mut buf = [0; 10];
    let len = json::get_by_key(&s, packed, &mut buf).unwrap();
    assert_eq!(&buf[..len], b"3");

    let key = JsonPath::from(".i[1].val");
    let any = s.mut_any_by_key(key.into_keys()).unwrap();

    let val: &mut dyn AddAssign<i32> = any.cast().unwrap();
    *val += 5;
    assert_eq!(*s.i[1].val, 3 + 5);

    hprintln!("success!");

    // exit QEMU
    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}
