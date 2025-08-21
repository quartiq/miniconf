use serde_reflection::{Samples, Tracer, TracerConfig};

use miniconf::graph::{Graph, Node};

mod common;

fn main() -> anyhow::Result<()> {
    let settings = common::Settings::new();

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
    graph
        .root()
        .visit(&mut vec![], &mut |_idx, node| {
            assert!(!matches!(node, Node::Leaf(None)));
            Ok::<_, ()>(())
        })
        .unwrap();

    // Dump graph and registry
    let registry = tracer.registry().unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&(graph.root(), &registry))?
    );

    Ok(())
}
