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

#[derive(Clone)]
pub(crate) struct SchemaDefs {
    defs: Vec<&'static Schema, MAX_SCHEMA_DEFS>,
}

impl SchemaDefs {
    pub(crate) fn new(root: &'static Schema) -> Result<Self, usize> {
        let mut defs = Vec::new();
        collect_defs(root, &mut defs)?;
        Ok(Self { defs })
    }

    pub(crate) fn len(&self) -> usize {
        self.defs.len()
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> Option<&'static Schema> {
        self.defs.last().copied()
    }

    fn id(&self, schema: &'static Schema) -> usize {
        self.defs
            .iter()
            .position(|candidate| ptr::eq(*candidate, schema))
            .unwrap()
    }

    fn get(&self, index: usize) -> &'static Schema {
        self.defs[index]
    }
}

fn collect_defs(
    schema: &'static Schema,
    defs: &mut Vec<&'static Schema, MAX_SCHEMA_DEFS>,
) -> Result<(), usize> {
    if defs.iter().any(|candidate| ptr::eq(*candidate, schema)) {
        return Ok(());
    }
    if let Some(internal) = schema.internal() {
        match internal {
            Internal::Named(children) => {
                for child in *children {
                    collect_defs(child.schema(), defs)?;
                }
            }
            Internal::Numbered(children) => {
                for child in *children {
                    collect_defs(child.schema(), defs)?;
                }
            }
            Internal::Homogeneous(child) => collect_defs(child.schema(), defs)?,
        }
    }
    if defs.iter().any(|candidate| ptr::eq(*candidate, schema)) {
        return Ok(());
    }
    defs.push(schema).map_err(|_| defs.len().saturating_add(1))
}

struct SchemaDef<'a> {
    defs: &'a SchemaDefs,
    schema: &'static Schema,
}

impl Serialize for SchemaDef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
        if !self.schema.node_meta().is_empty() {
            map.serialize_entry("m", self.schema.node_meta())?;
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
                    defs: self.defs,
                    internal,
                },
            )?;
        }
        map.end()
    }
}

struct SchemaChildren<'a> {
    defs: &'a SchemaDefs,
    internal: &'static Internal,
}

impl Serialize for SchemaChildren<'_> {
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
                        defs: self.defs,
                        children,
                    },
                )?;
            }
            Internal::Numbered(children) => {
                map.serialize_entry("k", "d")?;
                map.serialize_entry(
                    "c",
                    &NumberedChildren {
                        defs: self.defs,
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
                        defs: self.defs,
                        schema: child.schema(),
                        meta: maybe_meta(child.edge_meta()),
                    },
                )?;
            }
        }
        map.end()
    }
}

struct NamedChildren<'a> {
    defs: &'a SchemaDefs,
    children: &'static [miniconf::Named],
}

impl Serialize for NamedChildren<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.children.len()))?;
        for child in self.children {
            map.serialize_entry(
                child.name(),
                &ChildRef {
                    defs: self.defs,
                    schema: child.schema(),
                    meta: maybe_meta(child.edge_meta()),
                },
            )?;
        }
        map.end()
    }
}

struct NumberedChildren<'a> {
    defs: &'a SchemaDefs,
    children: &'static [miniconf::Numbered],
}

impl Serialize for NumberedChildren<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.children.len()))?;
        for child in self.children {
            seq.serialize_element(&ChildRef {
                defs: self.defs,
                schema: child.schema(),
                meta: maybe_meta(child.edge_meta()),
            })?;
        }
        seq.end()
    }
}

struct ChildRef<'a> {
    defs: &'a SchemaDefs,
    schema: &'static Schema,
    meta: Option<Meta>,
}

fn maybe_meta(meta: &Meta) -> Option<Meta> {
    if meta.is_empty() { None } else { Some(*meta) }
}

impl Serialize for ChildRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reference = SchemaRef(self.defs.id(self.schema));
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

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) struct SchemaPage {
    pub(crate) count: usize,
    pub(crate) len: usize,
}

pub(crate) fn serialize_schema_page(
    defs: &SchemaDefs,
    next: usize,
    payload: &mut [u8],
) -> Result<SchemaPage, usize> {
    let mut count = 0;
    let mut len = 0;

    while next + count < defs.len() {
        let id = next + count;
        let start = len;
        if payload.len().saturating_sub(start) < 2 {
            if start == 0 {
                return Err(id);
            }
            break;
        }
        let end = payload.len() - 1;
        let buf = &mut payload[start..end];
        let mut ser = serde_json_core::ser::Serializer::new(buf);
        let Ok(()) = (SchemaDef {
            defs,
            schema: defs.get(id),
        })
        .serialize(&mut ser) else {
            if start == 0 {
                return Err(id);
            }
            break;
        };
        len = start + ser.end();
        payload[len] = b'\n';
        len += 1;
        count += 1;
    }

    Ok(SchemaPage { count, len })
}

pub(crate) struct SchemaSync {
    pub(crate) defs: SchemaDefs,
    pub(crate) next: usize,
    pub(crate) page: usize,
    pub(crate) hash: u32,
}

impl SchemaSync {
    pub(crate) fn new(schema: &'static Schema) -> Self {
        Self {
            defs: SchemaDefs::new(schema).unwrap(),
            next: 0,
            page: 0,
            hash: <u32 as yafnv::Fnv>::OFFSET_BASIS,
        }
    }
}

pub(crate) type SettingsSync =
    NodeIter<ConstPath<String<MAX_TOPIC_LENGTH>, '/'>, { crate::MAX_DEPTH }>;
