//! Compact paged schema serialization.

use core::ptr;

use serde::{
    Serialize, Serializer,
    ser::{SerializeMap as _, SerializeSeq as _},
};

use crate::{Internal, Meta, Named, Numbered, Schema};

/// Ordered compact schema definitions for one schema tree.
#[derive(Clone)]
pub struct SchemaDefs<const N: usize> {
    defs: [&'static Schema; N],
    len: usize,
}

impl<const N: usize> SchemaDefs<N> {
    /// Collect schema definitions from a root schema.
    ///
    /// Definitions are stored in post-order so the root schema is the last definition.
    /// Returns the definition count that would have been needed if `N` is too small.
    pub fn new(root: &'static Schema) -> Result<Self, usize> {
        let mut defs = Self {
            defs: [&Schema::LEAF; N],
            len: 0,
        };
        defs.collect(root)?;
        Ok(defs)
    }

    /// Number of collected definitions.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether no definitions were collected.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Root schema definition.
    pub fn root(&self) -> Option<&'static Schema> {
        self.len.checked_sub(1).and_then(|index| self.get(index))
    }

    /// Schema definition by compact definition id.
    pub fn get(&self, index: usize) -> Option<&'static Schema> {
        (index < self.len).then(|| self.defs[index])
    }

    /// Compact serializable definition by id.
    pub fn definition(&self, id: usize) -> Option<SchemaDefinition<'_, N>> {
        Some(SchemaDefinition {
            defs: self,
            schema: self.get(id)?,
        })
    }

    fn id(&self, schema: &'static Schema) -> usize {
        self.defs[..self.len]
            .iter()
            .position(|candidate| ptr::eq(*candidate, schema))
            .unwrap()
    }

    fn contains(&self, schema: &'static Schema) -> bool {
        self.defs[..self.len]
            .iter()
            .any(|candidate| ptr::eq(*candidate, schema))
    }

    fn push(&mut self, schema: &'static Schema) -> Result<(), usize> {
        let Some(slot) = self.defs.get_mut(self.len) else {
            return Err(self.len.saturating_add(1));
        };
        *slot = schema;
        self.len += 1;
        Ok(())
    }

    fn collect(&mut self, schema: &'static Schema) -> Result<(), usize> {
        if self.contains(schema) {
            return Ok(());
        }
        if let Some(internal) = schema.internal() {
            match internal {
                Internal::Named(children) => {
                    for child in *children {
                        self.collect(child.schema())?;
                    }
                }
                Internal::Numbered(children) => {
                    for child in *children {
                        self.collect(child.schema())?;
                    }
                }
                Internal::Homogeneous(child) => self.collect(child.schema())?,
            }
        }
        if self.contains(schema) {
            return Ok(());
        }
        self.push(schema)
    }
}

/// One compact schema definition.
pub struct SchemaDefinition<'a, const N: usize> {
    defs: &'a SchemaDefs<N>,
    schema: &'static Schema,
}

impl<const N: usize> Serialize for SchemaDefinition<'_, N> {
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

struct SchemaChildren<'a, const N: usize> {
    defs: &'a SchemaDefs<N>,
    internal: &'static Internal,
}

impl<const N: usize> Serialize for SchemaChildren<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
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
                        meta: child.edge_meta(),
                    },
                )?;
            }
        }
        map.end()
    }
}

struct NamedChildren<'a, const N: usize> {
    defs: &'a SchemaDefs<N>,
    children: &'static [Named],
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
                    defs: self.defs,
                    schema: child.schema(),
                    meta: child.edge_meta(),
                },
            )?;
        }
        map.end()
    }
}

struct NumberedChildren<'a, const N: usize> {
    defs: &'a SchemaDefs<N>,
    children: &'static [Numbered],
}

impl<const N: usize> Serialize for NumberedChildren<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.children.len()))?;
        for child in self.children {
            seq.serialize_element(&ChildRef {
                defs: self.defs,
                schema: child.schema(),
                meta: child.edge_meta(),
            })?;
        }
        seq.end()
    }
}

struct ChildRef<'a, const N: usize> {
    defs: &'a SchemaDefs<N>,
    schema: &'static Schema,
    meta: &'a Meta,
}

impl<const N: usize> Serialize for ChildRef<'_, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reference = self.defs.id(self.schema);
        if self.meta.is_empty() {
            reference.serialize(serializer)
        } else {
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("r", &reference)?;
            map.serialize_entry("m", self.meta)?;
            map.end()
        }
    }
}

/// One compact schema page written into a caller-provided payload buffer.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SchemaPage {
    /// Number of schema definitions serialized into this page.
    pub count: usize,
    /// Number of bytes written into the payload buffer.
    pub len: usize,
}

/// Serialize compact schema definitions as newline-delimited JSON.
///
/// Starts with definition `next` and fills `payload` with as many whole definitions as fit.
/// Returns the id of the first oversized definition if no definition fits.
#[cfg(feature = "json-core")]
pub fn serialize_schema_page<const N: usize>(
    defs: &SchemaDefs<N>,
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
        let Some(definition) = defs.definition(id) else {
            break;
        };
        if definition.serialize(&mut ser).is_err() {
            if start == 0 {
                return Err(id);
            }
            break;
        }
        len = start + ser.end();
        payload[len] = b'\n';
        len += 1;
        count += 1;
    }

    Ok(SchemaPage { count, len })
}
