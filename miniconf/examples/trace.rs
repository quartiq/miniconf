use serde_json::to_string_pretty;
use serde_reflection::{Samples, Tracer, TracerConfig};

use miniconf::{json_schema::ReflectJsonSchema, trace::Types, TreeSchema};
use schemars::{generate::SchemaSettings, SchemaGenerator};

mod common;
use common::Settings;

/// Showcase for reflection and schema building
///
/// This

fn main() -> anyhow::Result<()> {
    println!("Schema:\n{}", to_string_pretty(Settings::SCHEMA)?);

    let mut types = Types::default();
    let mut tracer = Tracer::new(TracerConfig::default().is_human_readable(true));

    // Trace using TreeSerialize
    // If the value does not contain a value for a leaf node (e.g. KeyError::Absent),
    // it will leave the leaf node format unresolved.
    let mut samples = Samples::new();
    let settings = Settings::new();
    types
        .trace_values(&mut tracer, &mut samples, &settings)
        .unwrap();

    // Trace using TreeDeserialize assuming no samples are needed
    // If the Deserialize can't conjure up a value, it will leave the leaf node format unresolved.
    types.trace_types_simple(&mut tracer).unwrap();

    // No untraced Leaf nodes left
    // assert!(types.root().iter().all(|(_idx, fmt)| fmt.is_some()));
    println!("Leaves:\n{}", to_string_pretty(types.root())?);

    // Dump graph and registry
    let registry = tracer.registry().unwrap();
    println!("Registry:\n{}", to_string_pretty(&registry)?);

    let mut gen = SchemaGenerator::new(SchemaSettings::draft2020_12());
    let defs: Vec<_> = registry
        .iter()
        .map(|(name, value)| (name.clone(), value.json_schema(&mut gen).into()))
        .collect::<Vec<_>>();
    gen.definitions_mut().extend(defs);
    let mut root = types.root().json_schema(&mut gen).unwrap();
    root.insert("title".to_string(), "Miniconf example: Settings".into());
    root.insert("$defs".to_string(), gen.definitions().clone().into());
    if let Some(meta_schema) = gen.settings().meta_schema.as_deref() {
        root.insert("$schema".to_string(), meta_schema.into());
    }
    // use schemars::transform::{RecursiveTransform, Transform};
    // RecursiveTransform(miniconf::json_schema::unordered).transform(&mut root);
    // RecursiveTransform(miniconf::json_schema::strictify).transform(&mut root);
    println!("JSON Schema:\n{}", serde_json::to_string_pretty(&root)?);
    jsonschema::meta::validate(&root.to_value()).unwrap();

    Ok(())
}
