use core::marker::PhantomData;
use core::num::NonZero;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{
    Deserializer, EnumProgress, Format, FormatHolder, Samples, Serializer, Tracer, TracerConfig,
    Value,
};

use miniconf::{
    Error, IntoKeys, KeyLookup, Keys, Traversal, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

mod common;

/// Internal/leaf node metadata
#[derive(Clone, Serialize, PartialEq)]
pub enum Node {
    /// A terminal leaf node
    Leaf(Option<Format>),
    /// An internal node with named children
    Named(IndexMap<&'static str, Node>),
    /// An internal node with numbered children of homogenenous type
    Homogeneous {
        len: NonZero<usize>,
        item: Box<Node>,
    },
    /// An internal node with numbered children of heterogeneous type
    Numbered(Vec<Node>),
}

impl Walk for Node {
    fn internal(children: &[Self], lookup: &KeyLookup) -> Self {
        match lookup {
            KeyLookup::Named(names) => Self::Named(IndexMap::from_iter(
                names.iter().copied().zip(children.iter().cloned()),
            )),
            KeyLookup::Homogeneous(len) => Self::Homogeneous {
                len: *len,
                item: Box::new(children.first().unwrap().clone()),
            },
            KeyLookup::Numbered(_len) => Self::Numbered(children.to_vec()),
        }
    }

    fn leaf() -> Self {
        Self::Leaf(None)
    }
}

impl Node {
    /// Visit each node in the graph
    ///
    /// Pass the indices as well as the node by reference to the visitor
    ///
    /// Note that only the representative child will be visited for a
    /// homogeneous internal node.
    ///
    /// Top down, depth first.
    pub fn visit<F, E>(&self, root: &mut Vec<usize>, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&Vec<usize>, &Self) -> Result<(), E>,
    {
        func(root, self)?;
        match self {
            Self::Leaf(_) => {}
            Self::Homogeneous { item, .. } => {
                root.push(0); // at least one item guaranteed
                item.visit(root, func)?;
                root.pop();
            }
            Self::Named(map, ..) => {
                for (i, item) in map.values().enumerate() {
                    root.push(i);
                    item.visit(root, func)?;
                    root.pop();
                }
            }
            Self::Numbered(items) => {
                for (i, item) in items.iter().enumerate() {
                    root.push(i);
                    item.visit(root, func)?;
                    root.pop();
                }
            }
        }
        Ok(())
    }

    /// Visit each node in the graph mutably
    ///
    /// Pass the indices as well as the node by mutable reference to the visitor
    ///
    /// Note that only the representative child will be visited for a
    /// homogeneous internal node.
    ///
    /// top down, depth first.
    pub fn visit_mut<F, E>(&mut self, root: &mut Vec<usize>, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&Vec<usize>, &mut Self) -> Result<(), E>,
    {
        func(root, self)?;
        match self {
            Self::Leaf(_) => {}
            Self::Homogeneous { item, .. } => {
                root.push(0); // at least one item guaranteed
                item.visit_mut(root, func)?;
                root.pop();
            }
            Self::Named(map, ..) => {
                for (i, item) in map.values_mut().enumerate() {
                    root.push(i);
                    item.visit_mut(root, func)?;
                    root.pop();
                }
            }
            Self::Numbered(items) => {
                for (i, item) in items.iter_mut().enumerate() {
                    root.push(i);
                    item.visit_mut(root, func)?;
                    root.pop();
                }
            }
        }
        Ok(())
    }
}

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
pub struct Graph<T> {
    root: Node,
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
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// Visit all graph nodes by indices and node reference
    pub fn visit<F, E>(&self, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&Vec<usize>, &Node) -> Result<(), E>,
    {
        self.root.visit(&mut vec![], func)
    }

    /// Visit all graph nodes by indices and mutable node reference
    ///
    /// Not pub to uphold Graph<->T correctness
    fn visit_mut<F, E>(&mut self, func: &mut F) -> Result<(), E>
    where
        F: FnMut(&Vec<usize>, &mut Node) -> Result<(), E>,
    {
        self.root.visit_mut(&mut vec![], func)
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
        self.visit_mut(&mut |idx, node| {
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
        self.visit_mut(&mut |idx, node| {
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

    let mut graph = Graph::<common::Settings>::default();
    let mut tracer = Tracer::new(TracerConfig::default().is_human_readable(true));

    // Using TreeSerialize
    let mut samples = Samples::new();
    graph
        .trace_values(&mut tracer, &mut samples, &settings)
        .unwrap();

    // Using TreeDeserialize
    graph.trace_types_simple(&mut tracer).unwrap();

    // Dump graph and registry
    let registry = tracer.registry().unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&(graph.root(), &registry))?,
    );

    Ok(())
}
