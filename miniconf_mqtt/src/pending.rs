use core::{cell::Cell, ptr};

use heapless::String;
use miniconf::{Internal, Meta, MetaView, Named, NodeIter, Numbered, Path, Schema, SchemaIter};
use serde::{
    Serialize,
    ser::{SerializeMap as _, SerializeSeq, Serializer},
};

use crate::{MAX_PAYLOAD_LENGTH, MAX_TOPIC_LENGTH, SEPARATOR};

type LeafIter<const Y: usize> = NodeIter<Path<String<MAX_TOPIC_LENGTH>>, Y>;

const SCHEMA_ID_CACHE: usize = 8;

#[derive(Clone, Copy)]
struct CacheEntry {
    schema: *const Schema,
    id: usize,
}

pub(crate) struct SchemaIds<const Y: usize> {
    root: &'static Schema,
    entries: [Cell<Option<CacheEntry>>; SCHEMA_ID_CACHE],
    next: Cell<usize>,
}

impl<const Y: usize> SchemaIds<Y> {
    pub(crate) fn new(root: &'static Schema) -> Self {
        Self {
            root,
            entries: [const { Cell::new(None) }; SCHEMA_ID_CACHE],
            next: Cell::new(0),
        }
    }

    pub(crate) fn is_first_occurrence(
        &self,
        target: &'static Schema,
        state: &[usize; Y],
        depth: usize,
    ) -> bool {
        let target_state = &state[..depth];
        for entry in SchemaIter::new(self.root, [0; Y], 0) {
            if entry.depth() == depth
                && entry.path() == target_state
                && ptr::eq(entry.schema(), target)
            {
                return true;
            }
            if ptr::eq(entry.schema(), target) {
                return false;
            }
        }
        false
    }

    fn remember(&self, schema: &'static Schema, id: usize) {
        let next = self.next.get();
        self.entries[next].set(Some(CacheEntry {
            schema: schema as *const _,
            id,
        }));
        self.next.set((next + 1) % self.entries.len());
    }

    fn cached(&self, target: &'static Schema) -> Option<usize> {
        let target = target as *const _;
        self.entries
            .iter()
            .find_map(|entry| entry.get().filter(|entry| entry.schema == target))
            .map(|entry| entry.id)
    }

    fn compute_id(&self, target: &'static Schema) -> usize {
        let mut id = 0;
        for entry in SchemaIter::new(self.root, [0; Y], 0) {
            if ptr::eq(entry.schema(), target) {
                self.remember(entry.schema(), id);
                return id;
            }
            if self.is_first_occurrence(entry.schema(), &entry.state(), entry.depth()) {
                self.remember(entry.schema(), id);
                id += 1;
            }
        }
        unreachable!("schema node not reachable from root")
    }

    pub(crate) fn id_of(&self, schema: &'static Schema) -> usize {
        self.cached(schema)
            .unwrap_or_else(|| self.compute_id(schema))
    }
}

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

#[derive(Clone, Copy)]
pub(crate) struct CompactSchemaDef<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    schema: &'static Schema,
}

pub(crate) const fn compact_schema_def<'a, const Y: usize>(
    schema: &'static Schema,
    lookup: &'a SchemaIds<Y>,
) -> CompactSchemaDef<'a, Y> {
    CompactSchemaDef { lookup, schema }
}

impl<const Y: usize> Serialize for CompactSchemaDef<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        CompactSchemaBody {
            lookup: self.lookup,
            schema: self.schema,
        }
        .serialize(serializer)
    }
}

#[derive(Clone, Copy)]
struct CompactSchemaBody<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    schema: &'static Schema,
}

impl<const Y: usize> Serialize for CompactSchemaBody<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
        if let Some(meta) = self.schema.meta.as_ref() {
            map.serialize_entry("m", &MetaView(meta))?;
        }
        if let Some(sem) = self.schema.sem() {
            map.serialize_entry("s", sem)?;
        }
        if let Some(internal) = self.schema.internal.as_ref() {
            map.serialize_entry(
                "i",
                &CompactInternalView {
                    lookup: self.lookup,
                    internal,
                },
            )?;
        }
        map.end()
    }
}

#[derive(Clone, Copy)]
struct CompactInternalView<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    internal: &'static Internal,
}

impl<const Y: usize> Serialize for CompactInternalView<'_, Y> {
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
                    &CompactNamedChildren {
                        lookup: self.lookup,
                        children,
                    },
                )?;
            }
            Internal::Numbered(children) => {
                map.serialize_entry("k", "d")?;
                map.serialize_entry(
                    "c",
                    &CompactNumberedChildren {
                        lookup: self.lookup,
                        children,
                    },
                )?;
            }
            Internal::Homogeneous(child) => {
                map.serialize_entry("k", "h")?;
                map.serialize_entry("l", &child.len.get())?;
                map.serialize_entry(
                    "c",
                    &CompactHomogeneousChild {
                        lookup: self.lookup,
                        child,
                    },
                )?;
            }
        }
        map.end()
    }
}

#[derive(Clone, Copy)]
struct CompactNamedChildren<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    children: &'static [Named],
}

impl<const Y: usize> Serialize for CompactNamedChildren<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.children.len()))?;
        for child in self.children {
            map.serialize_entry(
                child.name,
                &CompactNamedChild {
                    lookup: self.lookup,
                    child,
                },
            )?;
        }
        map.end()
    }
}

#[derive(Clone, Copy)]
struct CompactNamedChild<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    child: &'static Named,
}

impl<const Y: usize> Serialize for CompactNamedChild<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_ref_with_meta(
            serializer,
            SchemaRef(self.lookup.id_of(self.child.schema)),
            self.child.meta,
        )
    }
}

#[derive(Clone, Copy)]
struct CompactNumberedChildren<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    children: &'static [Numbered],
}

impl<const Y: usize> Serialize for CompactNumberedChildren<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.children.len()))?;
        for child in self.children {
            seq.serialize_element(&CompactNumberedChild {
                lookup: self.lookup,
                child,
            })?;
        }
        seq.end()
    }
}

#[derive(Clone, Copy)]
struct CompactNumberedChild<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    child: &'static Numbered,
}

impl<const Y: usize> Serialize for CompactNumberedChild<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_ref_with_meta(
            serializer,
            SchemaRef(self.lookup.id_of(self.child.schema)),
            self.child.meta,
        )
    }
}

#[derive(Clone, Copy)]
struct CompactHomogeneousChild<'a, const Y: usize> {
    lookup: &'a SchemaIds<Y>,
    child: &'static miniconf::Homogeneous,
}

impl<const Y: usize> Serialize for CompactHomogeneousChild<'_, Y> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serialize_ref_with_meta(
            serializer,
            SchemaRef(self.lookup.id_of(self.child.schema)),
            self.child.meta,
        )
    }
}

fn serialize_ref_with_meta<S>(
    serializer: S,
    reference: SchemaRef,
    meta: Option<Meta>,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if meta.is_none() {
        return reference.serialize(serializer);
    }
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("r", &reference)?;
    if let Some(meta) = meta.as_ref() {
        map.serialize_entry("m", &MetaView(meta))?;
    }
    map.end()
}

#[allow(clippy::large_enum_variant)]
pub(crate) enum Pending<const Y: usize> {
    Idle,
    Schema {
        iter: SchemaIter<Y>,
        ids: SchemaIds<Y>,
        page: usize,
        hash: u32,
        carry: Option<String<MAX_PAYLOAD_LENGTH>>,
    },
    Settings {
        iter: LeafIter<Y>,
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
        // Schema publication is cold-path, so recomputing ids is preferable to keeping a
        // permanent definition table in client state.
        Self::Schema {
            iter: SchemaIter::new(schema, [0; Y], 0),
            ids: SchemaIds::new(schema),
            page: 0,
            hash: 0x811c9dc5,
            carry: None,
        }
    }

    pub(crate) fn settings(schema: &'static Schema) -> Self {
        Self::Settings {
            iter: NodeIter::new(schema, [0; Y], 0, SEPARATOR),
        }
    }
}
