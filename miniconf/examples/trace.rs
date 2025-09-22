//! Showcase for reflection and schema building

use miniconf::{json::to_json_value, json_schema::TreeJsonSchema};

mod common;
use common::Settings;

fn main() -> anyhow::Result<()> {
    let s = Settings::new();

    let value = to_json_value(&s)?;
    println!("JSON Tree:\n{}", serde_json::to_string_pretty(&value)?);

    let mut schema = TreeJsonSchema::new(Some(&s)).unwrap();

    schema
        .root
        .insert("title".to_string(), "Miniconf example: Settings".into());

    use schemars::transform::Transform;
    //miniconf::json_schema::Strictify.transform(&mut schema.root);
    miniconf::json_schema::AllowAbsent.transform(&mut schema.root);

    println!(
        "JSON Schema:\n{}",
        serde_json::to_string_pretty(&schema.root)?
    );

    jsonschema::meta::validate(schema.root.as_value()).unwrap();

    let validator = jsonschema::validator_for(schema.root.as_value())?;
    for e in validator.iter_errors(&value) {
        eprintln!("{e} {e:?}");
    }

    Ok(())
}
