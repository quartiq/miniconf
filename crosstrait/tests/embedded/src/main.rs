#![no_main]
#![no_std]
#![cfg_attr(feature = "used_linker", feature(used_with_arg))]
extern crate panic_semihosting;

use core::any::Any;

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};

use crosstrait::{register, Cast};
use miniconf::{self, Tree, TreeAny};

use core::ops::{AddAssign, SubAssign};
register! { i32 => dyn AddAssign<i32> }
register! { u32 => dyn SubAssign<u32> }

#[entry]
fn main() -> ! {
    assert_eq!(crosstrait::REGISTRY_KV.len(), 2);
    hprintln!(
        "registry RAM: {}",
        core::mem::size_of_val(&crosstrait::REGISTRY)
    );

    let mut a = 3i32;
    let v: &mut dyn AddAssign<i32> = (&mut a as &mut dyn Any).cast().unwrap();
    *v += 5;
    assert_eq!(a, 3 + 5);

    #[derive(Default, Tree)]
    struct Inner {
        val: i32,
    }

    #[derive(Default, Tree)]
    struct Settings {
        #[tree(depth = 1)]
        a: [i32; 2],
        #[tree(depth = 2)]
        i: [Inner; 3],
        #[tree(depth = 1)]
        b: Option<i32>,
    }
    let mut s = Settings::default();
    s.i[1].val = 3;
    let key = miniconf::Packed::new_from_lsb(0b1_01_01_0).unwrap();
    // let key = miniconf::JsonPath::from(".i[1].val");
    let any = s.mut_any_by_key(key).unwrap();
    let val: &mut dyn AddAssign<i32> = any.cast().unwrap();
    *val += 5;
    assert_eq!(s.i[1].val, 3 + 5);

    hprintln!("success!");

    // exit QEMU
    debug::exit(debug::EXIT_SUCCESS);

    loop {}
}
