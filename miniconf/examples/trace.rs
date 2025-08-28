use serde_json::to_string_pretty;
use serde_reflection::{Samples, Tracer, TracerConfig};

use miniconf::{trace::Types, TreeKey};

mod common;

fn main() -> anyhow::Result<()> {
    println!("Schema:\n{}", to_string_pretty(common::Settings::SCHEMA)?);

    let mut graph = Types::default();
    let mut tracer = Tracer::new(TracerConfig::default().is_human_readable(true));

    // Using TreeSerialize
    let mut samples = Samples::new();
    let settings = common::Settings::new();
    graph
        .trace_values(&mut tracer, &mut samples, &settings)
        .unwrap();

    // Using TreeDeserialize
    graph.trace_types_simple(&mut tracer).unwrap();

    // No untraced Leaf nodes left
    assert!(graph.leaves().iter().all(|(_idx, fmt)| fmt.is_some()));
    println!("Leaves:\n{}", to_string_pretty(graph.leaves())?);

    // Dump graph and registry
    let registry = tracer.registry().unwrap();
    println!("Registry:\n{}", to_string_pretty(&registry)?);

    Ok(())
}
