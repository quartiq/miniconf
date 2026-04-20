use core::ptr;

use heapless::{String, Vec};
use miniconf::{ConstPath, Internal, Meta, NodeIter, Schema};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap as _, SerializeSeq as _},
};

use crate::{MAX_SCHEMA_DEFS, MAX_TOPIC_LENGTH};

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
        if !self.schema.node_meta().is_empty() {
            let meta = self.schema.node_meta();
            map.serialize_entry("m", meta)?;
        }
        if let Some(sem) = self.schema.sem()
            && !sem.is_empty()
        {
            map.serialize_entry("s", sem)?;
        }
        if let Some(internal) = self.schema.internal() {
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
                map.serialize_entry("l", &child.len().get())?;
                map.serialize_entry(
                    "c",
                    &ChildRef {
                        emitted: self.emitted,
                        schema: child.schema(),
                        meta: maybe_meta(child.edge_meta()),
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
                child.name(),
                &ChildRef {
                    emitted: self.emitted,
                    schema: child.schema(),
                    meta: maybe_meta(child.edge_meta()),
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
                schema: child.schema(),
                meta: maybe_meta(child.edge_meta()),
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

fn maybe_meta(meta: &Meta) -> Option<Meta> {
    if meta.is_empty() { None } else { Some(*meta) }
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SchemaPage {
    pub(crate) count: usize,
    pub(crate) len: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum SchemaPageError {
    Oversized { id: usize },
}

struct SchemaPageBuilder<'a, const N: usize> {
    emitted: Vec<&'static Schema, N>,
    skip: usize,
    count: usize,
    payload: &'a mut [u8],
    len: usize,
    oversized: Option<SchemaPageError>,
    full: bool,
}

impl<'a, const N: usize> SchemaPageBuilder<'a, N> {
    fn new(skip: usize, payload: &'a mut [u8]) -> Self {
        Self {
            emitted: Vec::new(),
            skip,
            count: 0,
            payload,
            len: 0,
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
        if let Some(internal) = schema.internal() {
            match internal {
                Internal::Named(children) => {
                    for child in *children {
                        self.visit(child.schema());
                    }
                }
                Internal::Numbered(children) => {
                    for child in *children {
                        self.visit(child.schema());
                    }
                }
                Internal::Homogeneous(child) => self.visit(child.schema()),
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
        let start = self.len;
        if self.payload.len().saturating_sub(start) < 2 {
            if start == 0 {
                self.oversized = Some(SchemaPageError::Oversized { id });
            } else {
                self.full = true;
            }
            return;
        }
        let end = self.payload.len() - 1;
        let buf = &mut self.payload[start..end];
        let mut ser = serde_json_core::ser::Serializer::new(buf);
        let Ok(()) = SchemaDef {
            emitted: &self.emitted,
            schema,
        }
        .serialize(&mut ser) else {
            if start == 0 {
                self.oversized = Some(SchemaPageError::Oversized { id });
            } else {
                self.full = true;
            }
            return;
        };
        self.len = start + ser.end();
        self.emitted.push(schema).unwrap();
        self.payload[self.len] = b'\n';
        self.len += 1;
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
        if let Some(internal) = schema.internal() {
            match internal {
                Internal::Named(children) => {
                    for child in *children {
                        self.visit(child.schema());
                    }
                }
                Internal::Numbered(children) => {
                    for child in *children {
                        self.visit(child.schema());
                    }
                }
                Internal::Homogeneous(child) => self.visit(child.schema()),
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

pub(crate) fn serialize_schema_page(
    root: &'static Schema,
    skip: usize,
    payload: &mut [u8],
) -> Result<SchemaPage, SchemaPageError> {
    let mut builder = SchemaPageBuilder::<MAX_SCHEMA_DEFS>::new(skip, payload);
    builder.visit(root);
    if let Some(err) = builder.oversized {
        return Err(err);
    }
    Ok(SchemaPage {
        count: builder.count,
        len: builder.len,
    })
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum Pending {
    Idle,
    Schema {
        root: &'static Schema,
        defs: usize,
        next: usize,
        page: usize,
        hash: u32,
    },
    Settings {
        iter: NodeIter<ConstPath<String<MAX_TOPIC_LENGTH>, '/'>, { crate::MAX_DEPTH }>,
    },
}

impl Pending {
    pub(crate) const fn new() -> Self {
        Self::Idle
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::Idle;
    }

    pub(crate) fn schema(schema: &'static Schema) -> Self {
        Self::Schema {
            root: schema,
            defs: distinct_schema_defs(schema).unwrap(),
            next: 0,
            page: 0,
            hash: <u32 as yafnv::Fnv>::OFFSET_BASIS,
        }
    }

    pub(crate) fn settings(schema: &'static Schema) -> Self {
        Self::Settings {
            iter: NodeIter::new(schema, [0; crate::MAX_DEPTH], 0),
        }
    }
}
