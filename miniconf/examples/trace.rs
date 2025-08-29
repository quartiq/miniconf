use serde_json::to_string_pretty;
use serde_reflection::{Samples, Tracer, TracerConfig};

use miniconf::{
    schema::{self, ReflectJsonSchema},
    trace::Types,
    TreeKey,
};
use schemars::{
    generate::SchemaSettings,
    transform::{RecursiveTransform, Transform},
    SchemaGenerator,
};

mod common;

fn main() -> anyhow::Result<()> {
    println!("Schema:\n{}", to_string_pretty(common::Settings::SCHEMA)?);

    let mut types = Types::default();
    let mut tracer = Tracer::new(TracerConfig::default().is_human_readable(true));

    // Using TreeSerialize
    let mut samples = Samples::new();
    let settings = common::Settings::new();
    types
        .trace_values(&mut tracer, &mut samples, &settings)
        .unwrap();

    // Using TreeDeserialize
    types.trace_types_simple(&mut tracer).unwrap();

    // No untraced Leaf nodes left
    assert!(types.leaves().iter().all(|(_idx, fmt)| fmt.is_some()));
    println!("Leaves:\n{}", to_string_pretty(types.leaves())?);

    // Dump graph and registry
    let registry = tracer.registry().unwrap();
    println!("Registry:\n{}", to_string_pretty(&registry)?);

    let mut gen = SchemaGenerator::new(SchemaSettings::draft2020_12());
    let defs: Vec<_> = registry
        .iter()
        .map(|(name, value)| (name.clone(), value.json_schema(&mut gen).into()))
        .collect::<Vec<_>>();
    gen.definitions_mut().extend(defs);
    let mut root = types.json_schema(&mut gen).unwrap();
    root.insert("title".into(), "Settings".into());
    root.insert(
        "$id".into(),
        "https://quartiq.de/miniconf/example-settings".into(),
    );
    root.insert("$defs".into(), gen.definitions().clone().into());
    if let Some(meta_schema) = gen.settings().meta_schema.as_deref() {
        root.insert("$schema".into(), meta_schema.into());
    }
    // RecursiveTransform(schema::unordered).transform(&mut root);
    // RecursiveTransform(schema::strictify).transform(&mut root);
    println!("{}", serde_json::to_string_pretty(&root)?);
    // jsonschema::meta::validate(&root.to_value()).unwrap();
    Ok(())
}
