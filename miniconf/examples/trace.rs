use std::convert::Infallible;

use serde_json::to_string_pretty;
use serde_reflection::FormatHolder;

use miniconf::{TreeSchema, json_schema::TreeJsonSchema};

mod common;
use common::Settings;

/// Showcase for reflection and schema building

fn main() -> anyhow::Result<()> {
    println!("Schema:\n{}", to_string_pretty(Settings::SCHEMA)?);

    let mut schema = TreeJsonSchema::<common::Settings>::new().unwrap();

    // No untraced Leaf nodes left
    schema
        .types
        .root()
        .visit(&mut vec![0; 16], 0, &mut |_idx, (schema, fmt)| {
            assert!(!schema.is_leaf() || fmt.as_ref().is_some_and(|f| !f.is_unknown()));
            Ok::<_, Infallible>(())
        })
        .unwrap();
    println!("Leaves:\n{}", to_string_pretty(schema.types.root())?);

    // Dump graph and registry
    println!("Registry:\n{}", to_string_pretty(&schema.registry)?);

    schema
        .root
        .insert("title".to_string(), "Miniconf example: Settings".into());

    // use schemars::transform::{RecursiveTransform, Transform};
    // RecursiveTransform(miniconf::json_schema::strictify).transform(&mut root);
    println!(
        "JSON Schema:\n{}",
        serde_json::to_string_pretty(&schema.root)?
    );
    jsonschema::meta::validate(&schema.root.to_value()).unwrap();

    Ok(())
}
