use serde_reflection::{Samples, Tracer, TracerConfig};

use miniconf::trace::Graph;

mod common;

fn main() -> anyhow::Result<()> {
    let settings = common::Settings::new();
    // println!("{}", serde_json::to_string_pretty(<common::Settings as miniconf::TreeKey>::SCHEMA).unwrap());

    let mut graph = Graph::default();
    let mut tracer = Tracer::new(TracerConfig::default().is_human_readable(true));

    // Using TreeSerialize
    let mut samples = Samples::new();
    graph
        .trace_values(&mut tracer, &mut samples, &settings)
        .unwrap();

    // Using TreeDeserialize
    graph.trace_types_simple(&mut tracer).unwrap();

    // No untraced Leaf nodes left
    assert!(graph.leaves().iter().all(|(_idx, fmt)| fmt.is_some()));

    // Dump graph and registry
    let registry = tracer.registry().unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&(graph.leaves(), &registry))?
    );

    Ok(())
}
