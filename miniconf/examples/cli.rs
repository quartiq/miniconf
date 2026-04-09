use anyhow::Context;
use miniconf::{ConstPath, SerdeError, TreeSchema, ValueError, json_core};

mod common;
use common::Settings;

/// Simple command line interface example for miniconf
///
/// This exposes the leaf nodes in `Settings` as long options, parses the command line,
/// and then prints the settings struct as a list of option key-value pairs.
fn main() -> anyhow::Result<()> {
    let mut settings = Settings::new();
    settings.enable();
    // Parse args
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        let key = key.strip_prefix('-').context("stripping initial dash")?;
        let value = args.next().context("looking for value")?;
        json_core::set_by_key(&mut settings, ConstPath::<_, '-'>(key), value.as_bytes())
            .context("lookup/deserialize")?;
    }

    // Dump settings
    let mut buf = vec![0; 1024];
    const MAX_DEPTH: usize = Settings::SCHEMA.shape().max_depth;
    for item in Settings::SCHEMA.nodes::<ConstPath<String, '-'>, MAX_DEPTH>() {
        let key = item.unwrap();
        match json_core::get_by_key(&settings, &key, &mut buf[..]) {
            Ok(len) => {
                println!("-{} {}", key, core::str::from_utf8(&buf[..len]).unwrap());
            }
            Err(SerdeError::Value(ValueError::Absent)) => {
                let info = Settings::SCHEMA.get(&key).unwrap();
                println!("-{} absent (depth: {})", key, info.depth);
            }
            err => {
                err.unwrap();
            }
        }
    }

    Ok(())
}
