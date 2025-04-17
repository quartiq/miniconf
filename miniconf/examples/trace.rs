use core::num::NonZero;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde_reflection::{
    Deserializer, Format, FormatHolder, Registry, Samples, Serializer, TracerConfig, Value,
};

use miniconf::{
    Error, IntoKeys, KeyLookup, Keys, Traversal, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

mod common;

#[derive(Clone, Serialize, PartialEq)]
pub enum Node {
    Leaf(Option<Format>),
    Named(IndexMap<&'static str, Node>),
    Homogeneous {
        len: NonZero<usize>,
        item: Box<Node>,
    },
    Numbered(Vec<Node>),
}

impl Walk for Node {
    type Error = core::convert::Infallible;

    fn internal(children: &[Self], lookup: &KeyLookup) -> Result<Self, Self::Error> {
        Ok(match lookup {
            KeyLookup::Named(names) => Self::Named(IndexMap::from_iter(
                names.iter().copied().zip(children.iter().cloned()),
            )),
            KeyLookup::Homogeneous(len) => Self::Homogeneous {
                len: *len,
                item: Box::new(children.first().unwrap().clone()),
            },
            KeyLookup::Numbered(_len) => Self::Numbered(children.to_vec()),
        })
    }

    fn leaf() -> Self {
        Self::Leaf(None)
    }
}

impl Node {
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

pub struct Tracer(serde_reflection::Tracer);

impl Tracer {
    pub fn new(config: TracerConfig) -> Self {
        Self(serde_reflection::Tracer::new(config))
    }

    pub fn registry(self) -> serde_reflection::Result<Registry> {
        self.0.registry()
    }

    pub fn trace_value<T: TreeSerialize, K: Keys>(
        &mut self,
        samples: &mut Samples,
        value: &T,
        keys: K,
    ) -> Result<(Format, Value), Error<serde_reflection::Error>> {
        value.serialize_by_key(keys, Serializer::new(&mut self.0, samples))
    }

    pub fn trace_values<T>(
        &mut self,
        samples: &mut Samples,
        value: &T,
        root: &mut Node,
    ) -> Result<(), serde_reflection::Error>
    where
        T: TreeSerialize,
    {
        root.visit_mut(&mut vec![], &mut |keys, node| {
            if let Node::Leaf(format) = node {
                match self.trace_value(samples, value, keys.into_keys()) {
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

    pub fn trace_type_once<'de, T: TreeDeserialize<'de>, K: Keys>(
        &mut self,
        samples: &'de Samples,
        keys: K,
    ) -> Result<Format, Error<serde_reflection::Error>> {
        let mut format = Format::unknown();
        T::probe_by_key(keys, Deserializer::new(&mut self.0, samples, &mut format))?;
        format.reduce();
        Ok(format)
    }

    pub fn trace_type<'de, T: TreeDeserialize<'de>, K: Keys + Clone>(
        &mut self,
        samples: &'de Samples,
        keys: K,
    ) -> Result<Format, Error<serde_reflection::Error>> {
        loop {
            let format = self.trace_type_once::<T, _>(samples, keys.clone())?;
            if let Format::TypeName(name) = &format {
                if self.0.incomplete_enums.remove(name).is_some() {
                    // Restart the analysis to find more variants.
                    continue;
                }
            }
            return Ok(format);
        }
    }

    pub fn trace_types<'de, T>(
        &mut self,
        samples: &'de Samples,
        root: &mut Node,
    ) -> Result<(), serde_reflection::Error>
    where
        T: TreeDeserialize<'de>,
    {
        root.visit_mut(&mut vec![], &mut |keys, node| {
            if let Node::Leaf(format) = node {
                match self.trace_type::<T, _>(samples, keys.into_keys()) {
                    Ok(mut fmt) => {
                        fmt.reduce();
                        *format = Some(fmt);
                    }
                    Err(Error::Inner(_depth, e)) => Err(e)?,
                    _ => unreachable!(),
                }
            }
            Ok(())
        })
    }

    pub fn trace_types_simple<'de, T>(
        &mut self,
        root: &mut Node,
    ) -> Result<(), serde_reflection::Error>
    where
        T: TreeDeserialize<'de>,
    {
        static SAMPLES: Lazy<Samples> = Lazy::new(Samples::new);
        self.trace_types::<T>(&SAMPLES, root)
    }
}

fn main() -> anyhow::Result<()> {
    let settings = common::Settings::new();

    let mut root: Node = common::Settings::traverse_all()?;
    let mut tracer = Tracer::new(TracerConfig::default());

    // Trace values with TreeSerialize
    let mut samples = Samples::new();
    tracer
        .trace_values(&mut samples, &settings, &mut root)
        .unwrap();

    // Trace typs with TreeDeserialize
    tracer
        .trace_types_simple::<common::Settings>(&mut root)
        .unwrap();

    // Dump
    let registry = tracer.registry().unwrap();
    println!("{}", serde_json::to_string_pretty(&(&root, &registry))?,);

    Ok(())
}
