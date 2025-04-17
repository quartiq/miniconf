use core::num::NonZero;
use indexmap::IndexMap;
use std::hash::Hash;

use anyhow::Context;
use serde::Serialize;
use serde_reflection::{
    Deserializer, Format, FormatHolder, Registry, Samples, Serializer, TracerConfig, Value,
};

use miniconf::{
    Error, IntoKeys, KeyLookup, Path, Traversal, TreeDeserialize, TreeKey, TreeSerialize, Walk,
};

mod common;
use common::Settings;

#[derive(Clone, Serialize)]
#[serde(untagged)]
enum Node<'a> {
    Leaf(Option<&'a Format>),
    Named(IndexMap<&'static str, Node<'a>>),
    Homogeneous {
        len: NonZero<usize>,
        item: Box<Node<'a>>,
    },
    Numbered(Vec<Node<'a>>),
}

impl Walk for Node<'_> {
    type Error = core::convert::Infallible;

    fn internal(children: &[&Self], lookup: &KeyLookup) -> Result<Self, Self::Error> {
        Ok(match lookup {
            KeyLookup::Named(names) => Self::Named(IndexMap::from_iter(
                names.iter().copied().zip(children.iter().copied().cloned()),
            )),
            KeyLookup::Homogeneous(len) => Self::Homogeneous {
                len: *len,
                item: Box::new((*children.first().unwrap()).clone()),
            },
            KeyLookup::Numbered(_len) => {
                Self::Numbered(children.iter().copied().cloned().collect())
            }
        })
    }

    fn leaf() -> Self {
        Self::Leaf(None)
    }
}

impl Node<'_> {
    fn keys(&self, mut root: Vec<usize>) -> Vec<Vec<usize>> {
        match self {
            Self::Leaf(_) => vec![root],
            Self::Homogeneous { item, .. } => {
                root.push(0);
                item.keys(root)
            }
            Self::Named(map, ..) => map
                .values()
                .enumerate()
                .flat_map(|(i, child)| {
                    let mut idx = root.clone();
                    idx.push(i);
                    child.keys(idx)
                })
                .collect(),
            Self::Numbered(children) => children
                .iter()
                .enumerate()
                .flat_map(|(i, child)| {
                    let mut idx = root.clone();
                    idx.push(i);
                    child.keys(idx)
                })
                .collect(),
        }
    }
}
// fn lookup(&self) -> Option<KeyLookup> {
//     match self {
//         Self::Leaf => None,
//         Self::Homogeneous { len, .. } => Some(KeyLookup::Homogeneous(*len)),
//         Self::Named(_map, names) => Some(KeyLookup::Named(names)),
//         Self::Numbered(children) => {
//             Some(KeyLookup::Numbered(NonZero::new(children.len()).unwrap()))
//         }
//     }
// }

pub struct Tracer {
    tracer: serde_reflection::Tracer,
}

impl Tracer {
    pub fn new(config: TracerConfig) -> Self {
        Self {
            tracer: serde_reflection::Tracer::new(config),
        }
    }

    pub fn registry(self) -> serde_reflection::Result<Registry> {
        self.tracer.registry()
    }

    pub fn trace_value<T: TreeSerialize, K: IntoKeys>(
        &mut self,
        samples: &mut Samples,
        value: &T,
        keys: K,
    ) -> Result<(Format, Value), Error<serde_reflection::Error>> {
        value.serialize_by_key(keys.into_keys(), Serializer::new(&mut self.tracer, samples))
    }

    pub fn trace_values<T, I, K>(
        &mut self,
        samples: &mut Samples,
        value: &T,
        keys: I,
    ) -> Result<IndexMap<K, (Format, Value)>, Error<serde_reflection::Error>>
    where
        T: TreeSerialize,
        I: IntoIterator<Item = K>,
        K: IntoKeys + Clone + Hash + Eq,
    {
        keys.into_iter()
            .filter_map(
                |keys| match self.trace_value(samples, value, keys.clone()) {
                    Ok((mut format, value)) => {
                        format.reduce();
                        Some(Ok((keys, (format, value))))
                    }
                    Err(Error::Traversal(
                        Traversal::Absent(_depth) | Traversal::Access(_depth, _),
                    )) => None,
                    Err(e) => Some(Err(e)),
                },
            )
            .collect()
    }

    pub fn trace_type_once<'de, T: TreeDeserialize<'de>, K: IntoKeys>(
        &mut self,
        samples: &'de Samples,
        value: &mut T,
        keys: K,
    ) -> Result<Format, Error<serde_reflection::Error>> {
        let mut format = Format::unknown();
        value.deserialize_by_key(
            keys.into_keys(),
            Deserializer::new(&mut self.tracer, samples, &mut format),
        )?;
        format.reduce();
        Ok(format)
    }

    pub fn trace_type<'de, T: TreeDeserialize<'de>, K: IntoKeys + Clone>(
        &mut self,
        samples: &'de Samples,
        value: &mut T,
        keys: K,
    ) -> Result<Format, Error<serde_reflection::Error>> {
        loop {
            let format = self.trace_type_once(samples, value, keys.clone())?;
            if let Format::TypeName(name) = &format {
                if self.tracer.incomplete_enums.remove(name).is_some() {
                    // Restart the analysis to find more variants.
                    continue;
                }
            }
            return Ok(format);
        }
    }

    pub fn trace_types<'de, 'b, T, I, K>(
        &mut self,
        samples: &'de Samples,
        value: &mut T,
        keys: I,
    ) -> Result<IndexMap<K, Format>, Error<serde_reflection::Error>>
    where
        T: TreeDeserialize<'de>,
        I: IntoIterator<Item = K>,
        K: IntoKeys + Clone + Hash + Eq,
    {
        keys.into_iter()
            .filter_map(|keys| match self.trace_type(samples, value, keys.clone()) {
                Ok(mut format) => {
                    format.reduce();
                    Some(Ok((keys, format)))
                }
                Err(Error::Traversal(Traversal::Absent(_depth) | Traversal::Access(_depth, _))) => {
                    None
                }
                Err(e) => Some(Err(e)),
            })
            .collect()
    }
}

fn main() -> anyhow::Result<()> {
    let mut settings = Settings::default();
    settings.enable();

    let graph: Node = Settings::traverse_all()?;
    println!("{}", serde_json::to_string_pretty(&graph).context("graph")?);
    let keys = graph.keys(vec![]);
    let config = TracerConfig::default();
    let mut tracer = Tracer::new(config);
    let mut samples = Samples::new();
    let keys_slice = keys.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let _formats = tracer
        .trace_values(&mut samples, &settings, keys_slice.iter().copied())
        .unwrap(); // uncovered variants
    let formats = tracer
        .trace_types(&samples, &mut settings, keys_slice)
        .unwrap();
    let formats_paths: IndexMap<_, _> = formats
        .iter()
        .map(|(key, format)| {
            (
                Settings::transcode::<Path<String, '/'>, _>(*key)
                    .unwrap()
                    .0
                    .into_inner(),
                format,
            )
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&formats_paths).context("formats")?
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&tracer.registry().unwrap()).context("registry")?
    );
    Ok(())
}
