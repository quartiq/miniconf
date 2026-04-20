use core::ptr;

use heapless::{String, Vec};
use miniconf::{Internal, Meta, NodeIter, Path, Schema};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap as _, SerializeSeq as _},
};

use crate::{MAX_PAYLOAD_LENGTH, MAX_SCHEMA_DEFS, MAX_TOPIC_LENGTH, SEPARATOR, json::json_text};

#[derive(Clone, Copy)]
struct SchemaRef(usize);

impl Serialize for SchemaRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(self.0 as u64)
    }
}

struct SchemaDef<'a, const N: usize> {
    emitted: &'a Vec<&'static Schema, N>,
    schema: &'static Schema,
}

impl<const N: usize> Serialize for SchemaDef<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
        if let Some(meta) = self.schema.meta.as_ref() {
            map.serialize_entry("m", meta)?;
        }
        if let Some(sem) = self.schema.sem() {
            map.serialize_entry("s", sem)?;
        }
        if let Some(internal) = self.schema.internal.as_ref() {
            map.serialize_entry(
                "i",
                &SchemaChildren {
                    emitted: self.emitted,
                    internal,
                },
            )?;
        }
        map.end()
    }
}

struct SchemaChildren<'a, const N: usize> {
    emitted: &'a Vec<&'static Schema, N>,
    internal: &'static Internal,
}

impl<const N: usize> Serialize for SchemaChildren<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        match self.internal {
            Internal::Named(children) => {
                map.serialize_entry("k", "n")?;
                map.serialize_entry(
                    "c",
                    &NamedChildren {
                        emitted: self.emitted,
                        children,
                    },
                )?;
            }
            Internal::Numbered(children) => {
                map.serialize_entry("k", "d")?;
                map.serialize_entry(
                    "c",
                    &NumberedChildren {
                        emitted: self.emitted,
                        children,
                    },
                )?;
            }
            Internal::Homogeneous(child) => {
                map.serialize_entry("k", "h")?;
                map.serialize_entry("l", &child.len.get())?;
                map.serialize_entry(
                    "c",
                    &ChildRef {
                        emitted: self.emitted,
                        schema: child.schema,
                        meta: child.meta,
                    },
                )?;
            }
        }
        map.end()
    }
}

struct NamedChildren<'a, const N: usize> {
    emitted: &'a Vec<&'static Schema, N>,
    children: &'static [miniconf::Named],
}

impl<const N: usize> Serialize for NamedChildren<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.children.len()))?;
        for child in self.children {
            map.serialize_entry(
                child.name,
                &ChildRef {
                    emitted: self.emitted,
                    schema: child.schema,
                    meta: child.meta,
                },
            )?;
        }
        map.end()
    }
}

struct NumberedChildren<'a, const N: usize> {
    emitted: &'a Vec<&'static Schema, N>,
    children: &'static [miniconf::Numbered],
}

impl<const N: usize> Serialize for NumberedChildren<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.children.len()))?;
        for child in self.children {
            seq.serialize_element(&ChildRef {
                emitted: self.emitted,
                schema: child.schema,
                meta: child.meta,
            })?;
        }
        seq.end()
    }
}

struct ChildRef<'a, const N: usize> {
    emitted: &'a Vec<&'static Schema, N>,
    schema: &'static Schema,
    meta: Option<Meta>,
}

impl<const N: usize> Serialize for ChildRef<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reference = SchemaRef(emitted_id(self.emitted, self.schema));
        match self.meta.as_ref() {
            None => reference.serialize(serializer),
            Some(meta) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("r", &reference)?;
                map.serialize_entry("m", meta)?;
                map.end()
            }
        }
    }
}

fn emitted_id<const N: usize>(emitted: &Vec<&'static Schema, N>, schema: &'static Schema) -> usize {
    emitted
        .iter()
        .position(|candidate| ptr::eq(*candidate, schema))
        .unwrap()
}

pub(crate) enum SchemaPage {
    Done,
    Ready { count: usize },
    Oversized { id: usize },
}

struct SchemaPageBuilder<const N: usize> {
    emitted: Vec<&'static Schema, N>,
    skip: usize,
    count: usize,
    payload: String<MAX_PAYLOAD_LENGTH>,
    oversized: Option<usize>,
    full: bool,
}

impl<const N: usize> SchemaPageBuilder<N> {
    fn new(skip: usize) -> Self {
        Self {
            emitted: Vec::new(),
            skip,
            count: 0,
            payload: String::new(),
            oversized: None,
            full: false,
        }
    }

    fn contains(&self, schema: &'static Schema) -> bool {
        self.emitted
            .iter()
            .any(|candidate| ptr::eq(*candidate, schema))
    }

    fn visit(&mut self, schema: &'static Schema) {
        if self.full || self.oversized.is_some() || self.contains(schema) {
            return;
        }
        if let Some(internal) = schema.internal.as_ref() {
            match internal {
                Internal::Named(children) => {
                    for child in *children {
                        self.visit(child.schema);
                    }
                }
                Internal::Numbered(children) => {
                    for child in *children {
                        self.visit(child.schema);
                    }
                }
                Internal::Homogeneous(child) => self.visit(child.schema),
            }
        }
        if self.full || self.oversized.is_some() || self.contains(schema) {
            return;
        }
        if self.skip > 0 {
            self.emitted.push(schema).unwrap();
            self.skip -= 1;
            return;
        }

        let id = self.emitted.len();
        let Ok(line) = json_text::<MAX_PAYLOAD_LENGTH, _>(&SchemaDef {
            emitted: &self.emitted,
            schema,
        }) else {
            self.oversized = Some(id);
            return;
        };
        let need = line.len() + 1;
        if !self.payload.is_empty() && self.payload.len() + need > MAX_PAYLOAD_LENGTH {
            self.full = true;
            return;
        }
        self.emitted.push(schema).unwrap();
        self.payload.push_str(&line).unwrap();
        self.payload.push('\n').unwrap();
        self.count += 1;
    }
}

struct SchemaCounter<const N: usize> {
    emitted: Vec<&'static Schema, N>,
    overflowed: bool,
}

impl<const N: usize> SchemaCounter<N> {
    fn new() -> Self {
        Self {
            emitted: Vec::new(),
            overflowed: false,
        }
    }

    fn contains(&self, schema: &'static Schema) -> bool {
        self.emitted
            .iter()
            .any(|candidate| ptr::eq(*candidate, schema))
    }

    fn visit(&mut self, schema: &'static Schema) {
        if self.overflowed || self.contains(schema) {
            return;
        }
        if let Some(internal) = schema.internal.as_ref() {
            match internal {
                Internal::Named(children) => {
                    for child in *children {
                        self.visit(child.schema);
                    }
                }
                Internal::Numbered(children) => {
                    for child in *children {
                        self.visit(child.schema);
                    }
                }
                Internal::Homogeneous(child) => self.visit(child.schema),
            }
        }
        if self.contains(schema) {
            return;
        }
        if self.emitted.push(schema).is_err() {
            self.overflowed = true;
        }
    }
}

pub(crate) fn distinct_schema_defs(root: &'static Schema) -> Result<usize, usize> {
    let mut counter = SchemaCounter::<MAX_SCHEMA_DEFS>::new();
    counter.visit(root);
    if counter.overflowed {
        Err(counter.emitted.len() + 1)
    } else {
        Ok(counter.emitted.len())
    }
}

pub(crate) fn next_schema_page(
    root: &'static Schema,
    skip: usize,
    payload: &mut String<MAX_PAYLOAD_LENGTH>,
) -> SchemaPage {
    let mut builder = SchemaPageBuilder::<MAX_SCHEMA_DEFS>::new(skip);
    builder.visit(root);
    if let Some(id) = builder.oversized {
        return SchemaPage::Oversized { id };
    }
    if builder.count == 0 {
        return SchemaPage::Done;
    }
    *payload = builder.payload;
    SchemaPage::Ready {
        count: builder.count,
    }
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum Pending<const Y: usize> {
    Idle,
    Schema {
        root: &'static Schema,
        next: usize,
        page: usize,
        hash: u32,
    },
    Settings {
        iter: NodeIter<Path<String<MAX_TOPIC_LENGTH>>, Y>,
    },
}

impl<const Y: usize> Pending<Y> {
    pub(crate) const fn new() -> Self {
        Self::Idle
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::Idle;
    }

    pub(crate) fn schema(schema: &'static Schema) -> Self {
        Self::Schema {
            root: schema,
            next: 0,
            page: 0,
            hash: <u32 as yafnv::Fnv>::OFFSET_BASIS,
        }
    }

    pub(crate) fn settings(schema: &'static Schema) -> Self {
        Self::Settings {
            iter: NodeIter::new(schema, [0; Y], 0, SEPARATOR),
        }
    }
}
