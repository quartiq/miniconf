use anyhow::Context;
use miniconf::{json, IntoKeys, Keys, Path, SerdeError, TreeSchema, ValueError};

mod common;

// Simple command line interface example for miniconf.
// This exposes all leaf nodes as long options, parses the command line,
// and then prints the settings struct as a list of option key-value pairs.

fn main() -> anyhow::Result<()> {
    let mut settings = common::Settings::new();
    println!(
        "{}",
        serde_json::to_string_pretty(&common::Settings::SCHEMA)?
    );
    // Parse args
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        let key = key.strip_prefix('-').context("key must start with `-`")?;
        let value = args.next().context("missing value")?;
        json::set_by_key(&mut settings, Path::<_, '-'>(key), value.as_bytes())
            .context("lookup/deserialize")?;
    }

    // Dump settings
    let mut buf = vec![0; 1024];
    for item in common::Settings::SCHEMA.nodes::<Path<String, '-'>, 8>() {
        let key = item.unwrap();
        let mut k = key.into_keys().track();
        match json::get_by_key(&settings, &mut k, &mut buf[..]) {
            Ok(len) => {
                println!(
                    "-{} {}",
                    key.0.as_str(),
                    core::str::from_utf8(&buf[..len]).unwrap()
                );
            }
            Err(SerdeError::Value(ValueError::Absent)) => {
                println!("-{} absent (depth: {})", key.0.as_str(), k.depth);
            }
            Err(e) => panic!("{e:?}"),
        }
    }

    Ok(())
}
