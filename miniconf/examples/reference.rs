//! Export settings and construction defaults for miniconf-doc.

use miniconf::{json::to_json_value, json_schema::TreeJsonSchema};

mod common;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = common::Settings::new();
    let mut schema = TreeJsonSchema::new(Some(&settings))?.root;
    schema.insert("default".into(), to_json_value(&settings)?);
    println!("{}", serde_json::to_string_pretty(&schema)?);
    Ok(())
}
