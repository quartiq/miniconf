//! Use a compact key and a caller-owned payload buffer for one leaf.

use ::postcard::{de_flavors, ser_flavors};
use miniconf::{Packed, TreeSchema, postcard};

mod common;
use common::Settings;

fn main() {
    let mut source = Settings::new();
    source.output.dac[1] = 2048;
    let mut target = Settings::default();

    // Peers must agree on the schema: a Packed key encodes child indices.
    let key = Settings::SCHEMA
        .transcode::<Packed>("/output/dac/1")
        .unwrap();
    let mut buffer = [0; 8];
    let payload = postcard::get_by_key(&source, key, ser_flavors::Slice::new(&mut buffer)).unwrap();
    let remaining =
        postcard::set_by_key(&mut target, key, de_flavors::Slice::new(payload)).unwrap();
    assert!(remaining.is_empty());
    assert_eq!(target.output.dac[1], source.output.dac[1]);

    println!("key={} payload={payload:?}", key.into_lsb());
}
