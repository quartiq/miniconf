//! Schema tracing

use core::marker::PhantomData;

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{Format, FormatHolder, Samples, Tracer, Value};

use crate::{
    Internal, IntoKeys, Schema, SerdeError, TreeDeserialize, TreeSchema, TreeSerialize, ValueError,
};

/// Trace a leaf value
pub fn trace_value(
    tracer: &mut Tracer,
    samples: &mut Samples,
    keys: impl IntoKeys,
    value: impl TreeSerialize,
) -> Result<(Format, Value), SerdeError<serde_reflection::Error>> {
    let (mut format, sample) = value.serialize_by_key(
        keys.into_keys(),
        serde_reflection::Serializer::new(tracer, samples),
    )?;
    format.reduce();
    Ok((format, sample))
}

/// Trace a leaf type once
pub fn trace_type_once<'de, T: TreeDeserialize<'de>>(
    tracer: &mut Tracer,
    samples: &'de Samples,
    keys: impl IntoKeys,
) -> Result<Format, SerdeError<serde_reflection::Error>> {
    let mut format = Format::unknown();
    T::probe_by_key(
        keys.into_keys(),
        serde_reflection::Deserializer::new(tracer, samples, &mut format),
    )?;
    format.reduce();
    Ok(format)
}

/// Trace a leaf type until complete
pub fn trace_type<'de, T: TreeDeserialize<'de>>(
    tracer: &mut Tracer,
    samples: &'de Samples,
    keys: impl IntoKeys + Clone,
) -> Result<Format, SerdeError<serde_reflection::Error>> {
    loop {
        let format = trace_type_once::<T>(tracer, samples, keys.clone())?;
        if let Format::TypeName(name) = &format {
            if let Some(progress) = tracer.pend_enum(name) {
                debug_assert!(
                    !matches!(progress, serde_reflection::EnumProgress::Pending),
                    "failed to make progress tracing enum {name}"
                );
                // Restart the analysis to find more variants.
                continue;
            }
        }
        return Ok(format);
    }
}

/// A node in a graph
#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub struct Node<D> {
    /// Data associated witht this node
    pub data: D,
    /// Children of this node.
    ///
    /// Empty for leaf nodes.
    pub children: Vec<Node<D>>,
}

impl<D> Node<D> {
    /// Mutably visit all nodes
    pub fn visit<T, E>(
        &mut self,
        idx: &mut [usize],
        depth: usize,
        func: &mut impl FnMut(&[usize], &mut D) -> Result<T, E>,
    ) -> Result<T, E> {
        if depth < idx.len() {
            for (i, c) in self.children.iter_mut().enumerate() {
                idx[depth] = i;
                c.visit(idx, depth + 1, func)?;
            }
        }
        (*func)(&idx[..depth], &mut self.data)
    }
}

// Convert a Schema graph into a Node graph to be able to attach additional data to nodes.
impl<L: Default> From<&'static Schema> for Node<(&'static Schema, L)> {
    fn from(value: &'static Schema) -> Self {
        Self {
            data: (value, L::default()),
            children: value
                .internal
                .as_ref()
                .map(|internal| match internal {
                    Internal::Named(n) => n.iter().map(|n| Self::from(n.schema)).collect(),
                    Internal::Numbered(n) => n.iter().map(|n| Self::from(n.schema)).collect(),
                    Internal::Homogeneous(n) => vec![Self::from(n.schema)],
                })
                .unwrap_or_default(),
        }
    }
}

/// Graph of `Node`s for a Tree type
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Types<T> {
    pub(crate) root: Node<(&'static Schema, Option<Format>)>,
    _t: PhantomData<T>,
}

impl<T> Types<T> {
    /// Borrow the root node
    pub fn root(&self) -> &Node<(&'static Schema, Option<Format>)> {
        &self.root
    }
}

impl<T: TreeSchema> Default for Types<T> {
    fn default() -> Self {
        Self {
            root: Node::from(T::SCHEMA),
            _t: PhantomData,
        }
    }
}

impl<T> Types<T> {
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
        let mut idx = vec![0; T::SCHEMA.shape().max_depth];
        self.root.visit(&mut idx, 0, &mut |idx, (schema, format)| {
            if schema.is_leaf() {
                match trace_value(tracer, samples, idx, value) {
                    Ok((fmt, _value)) => {
                        *format = Some(fmt);
                    }
                    Err(SerdeError::Value(ValueError::Absent | ValueError::Access(_))) => {}
                    Err(SerdeError::Inner(e) | SerdeError::Finalization(e)) => Err(e)?,
                    // KeyError: Keys are all valid leaves by construction
                    Err(SerdeError::Value(ValueError::Key(_))) => unreachable!(),
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
        let mut idx = vec![0; T::SCHEMA.shape().max_depth];
        self.root.visit(&mut idx, 0, &mut |idx, (schema, format)| {
            if schema.is_leaf() {
                match trace_type::<T>(tracer, samples, idx) {
                    Ok(fmt) => {
                        *format = Some(fmt);
                    }
                    // probe access denied
                    Err(SerdeError::Value(ValueError::Access(_))) => {}
                    Err(SerdeError::Inner(e) | SerdeError::Finalization(e)) => Err(e)?,
                    // ValueError::Absent: Nodes are never absent on probe
                    // KeyError: Keys are all valid leaves by construction
                    Err(SerdeError::Value(ValueError::Absent | ValueError::Key(_))) => {
                        unreachable!()
                    }
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
