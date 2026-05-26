#![no_main]
#![no_std]

extern crate panic_halt;

use cortex_m_semihosting::{debug, hprintln};
use miniconf::{Internal, Meta, Named, Numbered, Schema, TreeSchema};
use miniconf_benchmark::settings::Settings;

const fn meta_bytes(meta: &Meta) -> usize {
    meta.items.len() * core::mem::size_of::<(&'static str, &'static str)>()
}

const fn schema_bytes(schema: &Schema) -> usize {
    let mut bytes = core::mem::size_of::<Schema>() + meta_bytes(schema.node_meta());
    if let Some(internal) = schema.internal() {
        match internal {
            Internal::Named(children) => {
                bytes += children.len() * core::mem::size_of::<Named>();
                let mut index = 0;
                while index < children.len() {
                    bytes += meta_bytes(children[index].edge_meta());
                    bytes += schema_bytes(children[index].schema());
                    index += 1;
                }
            }
            Internal::Numbered(children) => {
                bytes += children.len() * core::mem::size_of::<Numbered>();
                let mut index = 0;
                while index < children.len() {
                    bytes += meta_bytes(children[index].edge_meta());
                    bytes += schema_bytes(children[index].schema());
                    index += 1;
                }
            }
            Internal::Homogeneous(child) => {
                bytes += meta_bytes(child.edge_meta());
                bytes += schema_bytes(child.schema());
            }
        }
    }
    bytes
}

const SETTINGS_SCHEMA_BYTES: usize = schema_bytes(Settings::SCHEMA);

#[cortex_m_rt::entry]
fn main() -> ! {
    hprintln!("RESULT schema_bytes={}", SETTINGS_SCHEMA_BYTES);
    debug::exit(debug::EXIT_SUCCESS);
    loop {
        core::hint::spin_loop();
    }
}
