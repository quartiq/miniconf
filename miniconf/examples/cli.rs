use anyhow::{Context, Result};
use miniconf::{Error, JsonCoreSlash, Path, Traversal, Tree, TreeKey};
use serde::{Deserialize, Serialize};

// Either/Inner/Settings are straight from README.md

#[derive(Deserialize, Serialize, Default)]
enum Either {
    #[default]
    Bad,
    Good,
}

#[derive(Deserialize, Serialize, Default, Tree)]
struct Inner {
    a: i32,
    b: i32,
}

#[derive(Tree, Default)]
struct Settings {
    foo: bool,
    enum_: Either,
    struct_: Inner,
    array: [i32; 2],
    option: Option<i32>,

    #[tree(skip)]
    #[allow(unused)]
    skipped: (),

    #[tree(depth = 1)]
    struct_tree: Inner,
    #[tree(depth = 1)]
    array_tree: [i32; 2],
    #[tree(depth = 2)]
    array_tree2: [Inner; 2],

    #[tree(depth = 1)]
    option_tree: Option<i32>,
    #[tree(depth = 2)]
    option_tree2: Option<Inner>,
    #[tree(depth = 3)]
    array_option_tree: [Option<Inner>; 2],
}

fn main() -> Result<()> {
    let mut settings = Settings::default();
    settings.array_option_tree[1] = Some(Default::default());

    // Parse args
    let mut args = std::env::args().skip(1);
    while let Some(key) = args.next() {
        let key = key.strip_prefix('-').context("key must start with `-`")?;
        let value = args.next().context("missing value")?;
        settings
            .set_json_by_key(&Path::<_, '-'>(key), value.as_bytes())
            .map_err(anyhow::Error::msg)
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
