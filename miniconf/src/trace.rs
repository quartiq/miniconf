//! Schema tracing

use core::marker::PhantomData;

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{Format, FormatHolder, Samples, Tracer, Value};

use crate::{
    Internal, IntoKeys, SerDeError, TreeDeserialize, TreeSchema, TreeSerialize, ValueError,
};

/// Trace a leaf value
pub fn trace_value(
    tracer: &mut Tracer,
    samples: &mut Samples,
    keys: impl IntoKeys,
    value: &impl TreeSerialize,
) -> Result<(Format, Value), SerDeError<serde_reflection::Error>> {
    value.serialize_by_key(
        keys.into_keys(),
        serde_reflection::Serializer::new(tracer, samples),
    )
}

/// Trace a leaf type once
pub fn trace_type_once<'de, T: TreeDeserialize<'de>>(
    tracer: &mut Tracer,
    samples: &'de Samples,
    keys: impl IntoKeys,
) -> Result<Format, SerDeError<serde_reflection::Error>> {
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
) -> Result<Format, SerDeError<serde_reflection::Error>> {
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

#[derive(Clone, Debug, PartialEq, PartialOrd, Hash, Serialize)]
pub enum Node<T> {
    Internal(Vec<Node<T>>),
    Leaf(Option<T>),
}

impl<T> Node<T> {
    pub fn get(&self, idx: &[usize]) -> &Option<T> {
        match self {
            Self::Internal(i) => i[idx[0]].get(&idx[1..]),
            Self::Leaf(t) => {
                debug_assert!(idx.is_empty());
                t
            }
        }
    }

    pub fn get_mut(&mut self, idx: &[usize]) -> &mut Option<T> {
        match self {
            Self::Internal(i) => i[idx[0]].get_mut(&idx[1..]),
            Self::Leaf(t) => {
                debug_assert!(idx.is_empty());
                t
            }
        }
    }

    pub fn visit_leaves<E>(
        &mut self,
        idx: &mut [usize],
        depth: usize,
        func: &mut impl FnMut(&[usize], &mut Option<T>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::Leaf(t) => func(&idx[..depth], t),
            Self::Internal(c) => {
                for (i, c) in c.iter_mut().enumerate() {
                    idx[depth] = i;
                    c.visit_leaves(idx, depth + 1, func)?;
                }
                Ok(())
            }
        }
    }
}

impl<T> From<&crate::Schema> for Node<T> {
    fn from(value: &crate::Schema) -> Self {
        match value.internal.as_ref() {
            Some(internal) => Self::Internal(match internal {
                Internal::Named(n) => n.iter().map(|n| n.schema.into()).collect(),
                Internal::Numbered(n) => n.iter().map(|n| n.schema.into()).collect(),
                Internal::Homogeneous(n) => vec![n.schema.into()],
            }),
            None => Self::Leaf(None),
        }
    }
}

/// Graph of `Node` for a Tree type
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Types<T, N> {
    pub(crate) root: Node<N>,
    _t: PhantomData<T>,
}

impl<T, N> Types<T, N> {
    pub fn root(&self) -> &Node<N> {
        &self.root
    }
}

impl<T: TreeSchema, N> Default for Types<T, N> {
    fn default() -> Self {
        Self {
            root: T::SCHEMA.into(),
            _t: PhantomData,
        }
    }
}

impl<T> Types<T, Format> {
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
        self.root.visit_leaves(&mut idx[..], 0, &mut |idx, format| {
            match trace_value(tracer, samples, idx, value) {
                Ok((mut fmt, _value)) => {
                    fmt.reduce();
                    *format = Some(fmt);
                }
                Err(SerDeError::Value(ValueError::Absent | ValueError::Access(_))) => {}
                Err(SerDeError::Inner(e)) => Err(e)?,
                _ => unreachable!(),
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
        self.root.visit_leaves(&mut idx[..], 0, &mut |idx, format| {
            match trace_type::<T>(tracer, samples, idx) {
                Ok(mut fmt) => {
                    fmt.reduce();
                    *format = Some(fmt);
                }
                Err(SerDeError::Value(ValueError::Access(_msg))) => {
                    // probe access denied
                }
                Err(SerDeError::Inner(e) | SerDeError::Finalization(e)) => Err(e)?,
                _ => unreachable!(),
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
