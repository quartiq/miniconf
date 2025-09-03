//! Schema tracing

use core::marker::PhantomData;

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{Format, FormatHolder, Samples, Tracer, Value};

use crate::{
    Internal, IntoKeys, Schema, SerDeError, TreeDeserialize, TreeSchema, TreeSerialize, ValueError,
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
pub struct Node<D> {
    pub data: D,
    pub children: Vec<Node<D>>,
}

impl<D> Node<D> {
    pub fn visit<E>(
        &mut self,
        idx: &mut [usize],
        depth: usize,
        func: &mut impl FnMut(&[usize], &mut D) -> Result<(), E>,
    ) -> Result<(), E> {
        for (i, c) in self.children.iter_mut().enumerate() {
            idx[depth] = i;
            c.visit(idx, depth + 1, func)?;
        }
        func(&idx[..depth], &mut self.data)
    }
}

impl<L> From<&'static Schema> for Node<(&'static Schema, Option<L>)> {
    fn from(value: &'static Schema) -> Self {
        Self {
            data: (value, None),
            children: if let Some(internal) = value.internal.as_ref() {
                match internal {
                    Internal::Named(n) => n.iter().map(|n| n.schema.into()).collect(),
                    Internal::Numbered(n) => n.iter().map(|n| n.schema.into()).collect(),
                    Internal::Homogeneous(n) => vec![n.schema.into()],
                }
            } else {
                vec![]
            },
        }
    }
}

/// Graph of `Node` for a Tree type
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Types<T> {
    pub(crate) root: Node<(&'static Schema, Option<Format>)>,
    _t: PhantomData<T>,
}

impl<T> Types<T> {
    pub fn root(&self) -> &Node<(&'static Schema, Option<Format>)> {
        &self.root
    }
}

impl<T: TreeSchema> Default for Types<T> {
    fn default() -> Self {
        Self {
            root: T::SCHEMA.into(),
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
        self.root
            .visit(&mut idx[..], 0, &mut |idx, (schema, format)| {
                if schema.is_leaf() {
                    match trace_value(tracer, samples, idx, value) {
                        Ok((mut fmt, _value)) => {
                            fmt.reduce();
                            *format = Some(fmt);
                        }
                        Err(SerDeError::Value(ValueError::Absent | ValueError::Access(_))) => {}
                        Err(SerDeError::Inner(e)) => Err(e)?,
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
        let mut idx = vec![0; T::SCHEMA.shape().max_depth];
        self.root
            .visit(&mut idx[..], 0, &mut |idx, (schema, format)| {
                if schema.is_leaf() {
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
