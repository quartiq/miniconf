#![no_main]
#![no_std]

extern crate panic_halt;

#[cortex_m_rt::entry]
fn main() -> ! {
    miniconf_benchmark::run_engine::<miniconf_benchmark::miniconf_engine::Engine>()
}
