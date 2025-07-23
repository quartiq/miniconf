use core::marker::PhantomData;

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{
    Deserializer, EnumProgress, Format, FormatHolder, Samples, Serializer, Tracer, TracerConfig,
    Value,
};

use miniconf::{Error, IntoKeys, Keys, Traversal, TreeDeserialize, TreeKey, TreeSerialize};

mod common;
mod node;
use node::Node;

/// Trace a leaf value
pub fn trace_value<T: TreeSerialize, K: Keys>(
    tracer: &mut Tracer,
    samples: &mut Samples,
    keys: K,
    value: &T,
) -> Result<(Format, Value), Error<serde_reflection::Error>> {
    value.serialize_by_key(keys, Serializer::new(tracer, samples))
}

/// Trace a leaf type once
pub fn trace_type_once<'de, T: TreeDeserialize<'de>, K: Keys>(
    tracer: &mut Tracer,
    samples: &'de Samples,
    keys: K,
) -> Result<Format, Error<serde_reflection::Error>> {
    let mut format = Format::unknown();
    T::probe_by_key(keys, Deserializer::new(tracer, samples, &mut format))?;
    format.reduce();
    Ok(format)
}

/// Trace a leaf type until complete
pub fn trace_type<'de, T: TreeDeserialize<'de>, K: Keys + Clone>(
    tracer: &mut Tracer,
    samples: &'de Samples,
    keys: K,
) -> Result<Format, Error<serde_reflection::Error>> {
    loop {
        let format = trace_type_once::<T, _>(tracer, samples, keys.clone())?;
        if let Format::TypeName(name) = &format {
            if let Some(progress) = tracer.pend_enum(name) {
                debug_assert!(
                    !matches!(progress, EnumProgress::Pending),
                    "failed to make progress tracing enum {name}"
                );
                // Restart the analysis to find more variants.
                continue;
            }
        }
        return Ok(format);
    }
}

/// Graph of `Node` for a Tree type
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Graph<T> {
    root: Node<Format>,
    _t: PhantomData<T>,
}

impl<T: TreeKey> Default for Graph<T> {
    fn default() -> Self {
        Self {
            root: T::traverse_all(),
            _t: PhantomData,
        }
    }
}

impl<T> Graph<T> {
    /// Return a reference to the root node
    pub fn root(&self) -> &Node<Format> {
        &self.root
    }

    /// Trace all leaf values
    pub fn trace_values(
        &mut self,
        tracer: &mut Tracer,
        samples: &mut Samples,
        value: &T,
    ) -> Result<(), serde_reflection::Error>
    where
        T: TreeSerialize,
    {
        self.root.visit_mut(&mut vec![], &mut |idx, node| {
            if let Node::Leaf(format) = node {
                match trace_value(tracer, samples, idx.into_keys(), value) {
                    Ok((mut fmt, _value)) => {
                        fmt.reduce();
                        *format = Some(fmt);
                    }
                    Err(Error::Traversal(
                        Traversal::Absent(_depth) | Traversal::Access(_depth, _),
                    )) => {}
                    Err(Error::Inner(_depth, e)) => Err(e)?,
                    _ => unreachable!(),
                }
            }
            Ok(())
        })
    }

    /// Trace all leaf types until complete
    pub fn trace_types<'de>(
        &mut self,
        tracer: &mut Tracer,
        samples: &'de Samples,
    ) -> Result<(), serde_reflection::Error>
    where
        T: TreeDeserialize<'de>,
    {
        self.root.visit_mut(&mut vec![], &mut |idx, node| {
            if let Node::Leaf(format) = node {
                match trace_type::<T, _>(tracer, samples, idx.into_keys()) {
                    Ok(mut fmt) => {
                        fmt.reduce();
                        *format = Some(fmt);
                    }
                    Err(Error::Traversal(Traversal::Access(_depth, msg))) => {
                        Err(serde_reflection::Error::DeserializationError(msg))?
                    }
                    Err(Error::Inner(_depth, e)) => Err(e)?,
                    _ => unreachable!(),
                }
            }
            Ok(())
        })
    }

    /// Trace all leaf types assuming no samples are needed
    pub fn trace_types_simple<'de>(
        &mut self,
        tracer: &mut Tracer,
    ) -> Result<(), serde_reflection::Error>
    where
        T: TreeDeserialize<'de>,
    {
        static SAMPLES: Lazy<Samples> = Lazy::new(Samples::new);
        self.trace_types(tracer, &SAMPLES)
    }
}

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
