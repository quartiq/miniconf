//! Schema tracing

use core::marker::PhantomData;

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{
    Deserializer, EnumProgress, Format, FormatHolder, Samples, Serializer, Tracer, Value,
};

use crate::{Error, Keys, Packed, Traversal, TreeDeserialize, TreeKey, TreeSerialize};

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
pub struct Graph<T, N> {
    pub(crate) leaves: Vec<(Packed, Option<N>)>,
    _t: PhantomData<T>,
}

impl<T, N> Graph<T, N> {
    pub fn leaves(&self) -> &Vec<(Packed, Option<N>)> {
        &self.leaves
    }
}

impl<T: TreeKey, N> Default for Graph<T, N> {
    fn default() -> Self {
        let mut idx = vec![0; T::SCHEMA.metadata().max_depth];
        let mut leaves = Vec::new();
        T::SCHEMA
            .visit(&mut idx, 0, &mut |idx, schema| {
                if schema.internal.is_none() {
                    let (p, _) = schema.transcode(idx).unwrap();
                    println!("{p:?} {idx:?}");
                    leaves.push((p, None));
                }
                Ok::<_, ()>(())
            })
            .unwrap();
        Self {
            leaves,
            _t: PhantomData,
        }
    }
}

impl<T> Graph<T, Format> {
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
        for (idx, format) in self.leaves.iter_mut() {
            match trace_value(tracer, samples, *idx, value) {
                Ok((mut fmt, _value)) => {
                    fmt.reduce();
                    *format = Some(fmt);
                }
                Err(Error::Traversal(Traversal::Absent(_depth) | Traversal::Access(_depth, _))) => {
                }
                Err(Error::Inner(_depth, e)) => Err(e)?,
                _ => unreachable!(),
            }
        }
        Ok(())
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
        for (idx, format) in self.leaves.iter_mut() {
            match trace_type::<T, _>(tracer, samples, *idx) {
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
