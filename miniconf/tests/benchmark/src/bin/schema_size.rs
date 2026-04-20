#![no_main]
#![no_std]

extern crate panic_halt;

use cortex_m_semihosting::{debug, hprintln};
use miniconf::TreeSchema;
use miniconf_benchmark::settings::Settings;

const fn meta_bytes(meta: &miniconf::Meta) -> usize {
    meta.items.len() * core::mem::size_of::<(&'static str, &'static str)>()
}

const fn schema_bytes(schema: &miniconf::Schema) -> usize {
    let mut bytes = core::mem::size_of::<miniconf::Schema>() + meta_bytes(schema.node_meta());
    if let Some(internal) = schema.internal() {
        match internal {
            miniconf::Internal::Named(children) => {
                bytes += children.len() * core::mem::size_of::<miniconf::Named>();
                let mut index = 0;
                while index < children.len() {
                    bytes += meta_bytes(children[index].edge_meta());
                    bytes += schema_bytes(children[index].schema());
                    index += 1;
                }
            }
            miniconf::Internal::Numbered(children) => {
                bytes += children.len() * core::mem::size_of::<miniconf::Numbered>();
                let mut index = 0;
                while index < children.len() {
                    bytes += meta_bytes(children[index].edge_meta());
                    bytes += schema_bytes(children[index].schema());
                    index += 1;
                }
            }
            miniconf::Internal::Homogeneous(child) => {
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
