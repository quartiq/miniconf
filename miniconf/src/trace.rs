//! Schema tracing

use core::{convert::Infallible, marker::PhantomData};

use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{
    Deserializer, EnumProgress, Format, FormatHolder, Samples, Serializer, Tracer, Value,
};

use crate::{Keys, Packed, SerDeError, TreeDeserialize, TreeKey, TreeSerialize, ValueError};

/// Trace a leaf value
pub fn trace_value(
    tracer: &mut Tracer,
    samples: &mut Samples,
    keys: impl Keys,
    value: &impl TreeSerialize,
) -> Result<(Format, Value), SerDeError<serde_reflection::Error>> {
    value.serialize_by_key(keys, Serializer::new(tracer, samples))
}

/// Trace a leaf type once
pub fn trace_type_once<'de, T: TreeDeserialize<'de>>(
    tracer: &mut Tracer,
    samples: &'de Samples,
    keys: impl Keys,
) -> Result<Format, SerDeError<serde_reflection::Error>> {
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
) -> Result<Format, SerDeError<serde_reflection::Error>> {
    loop {
        let format = trace_type_once::<T>(tracer, samples, keys.clone())?;
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
pub struct Types<T, N> {
    pub(crate) leaves: Vec<(Packed, Option<N>)>,
    _t: PhantomData<T>,
}

impl<T, N> Types<T, N> {
    pub fn leaves(&self) -> &Vec<(Packed, Option<N>)> {
        &self.leaves
    }
}

impl<T: TreeKey, N> Default for Types<T, N> {
    fn default() -> Self {
        let meta = T::SCHEMA.metadata();
        let mut idx = vec![0; meta.max_depth];
        let mut leaves = Vec::with_capacity(meta.count.get());
        T::SCHEMA
            .visit_schema(&mut idx, 0, &mut |idx, schema| {
                if schema.is_leaf() {
                    let p = T::SCHEMA.transcode(idx).unwrap();
                    leaves.push((p, None));
                }
                Ok::<_, Infallible>(())
            })
            .unwrap();
        Self {
            leaves,
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
        for (idx, format) in self.leaves.iter_mut() {
            match trace_value(tracer, samples, *idx, value) {
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
                Err(SerDeError::Value(ValueError::Access(msg))) => {
                    Err(serde_reflection::Error::DeserializationError(msg))?
                }
                Err(SerDeError::Inner(e)) => Err(e)?,
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
