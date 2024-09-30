use anyhow::Context;
use miniconf::{Error, JsonCoreSlash, Path, Traversal, TreeKey};

mod common;
use common::Settings;

// Simple command line interface example for miniconf.
// This exposes all leaf nodes as long options, parses the command line,
// and then prints the settings struct as a list of option key-value pairs.

fn main() -> anyhow::Result<()> {
    let mut settings = Settings::default();
    settings.enable();

    // Parse args
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        let key = key.strip_prefix('-').context("key must start with `-`")?;
        let value = args.next().context("missing value")?;
        settings
            .set_json_by_key(&Path::<_, '-'>(key), value.as_bytes())
            .context("lookup/deserialize")?;
    }

    // Dump settings
    let mut buf = vec![0; 1024];
    for (key, _node) in Settings::nodes::<Path<String, '-'>>().map(Result::unwrap) {
        match settings.get_json_by_key(&key, &mut buf[..]) {
            Ok(len) => {
                println!(
                    "-{} {}",
                    key.as_str(),
                    core::str::from_utf8(&buf[..len]).unwrap()
                );
            }
            Err(Error::Traversal(Traversal::Absent(depth))) => {
                println!("-{} absent (depth: {depth})", key.as_str());
            }
            Err(e) => panic!("{e:?}"),
        }
    }

    Ok(())
}
