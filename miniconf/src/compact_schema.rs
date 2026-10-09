//! Compact paged schema serialization.

use core::ptr;

use serde::{Serialize, Serializer, ser::SerializeMap as _};

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
    /// The schema must be acyclic. If `N` is too small, returns a lower bound on
    /// the required definition count.
    pub fn new(root: &'static Schema) -> Result<Self, usize> {
        let mut defs = [&Schema::LEAF; N];
        let len = collect(root, &mut defs, 0)?;
        Ok(Self { defs, len })
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
    pub fn definition(&self, id: usize) -> Option<SchemaDefinition<'_>> {
        Some(SchemaDefinition {
            defs: &self.defs[..self.len],
            schema: self.get(id)?,
        })
    }
}

fn collect(
    schema: &'static Schema,
    defs: &mut [&'static Schema],
    mut len: usize,
) -> Result<usize, usize> {
    if defs[..len]
        .iter()
        .any(|candidate| ptr::eq(*candidate, schema))
    {
        return Ok(len);
    }
    if let Some(internal) = schema.internal() {
        for child in internal.schemata() {
            len = collect(child, defs, len)?;
        }
    }
    let Some(slot) = defs.get_mut(len) else {
        return Err(len.saturating_add(1));
    };
    *slot = schema;
    Ok(len + 1)
}

/// One compact schema definition.
pub struct SchemaDefinition<'a> {
    defs: &'a [&'static Schema],
    schema: &'static Schema,
}

impl Serialize for SchemaDefinition<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let meta = self.schema.node_meta();
        let sem = self.schema.sem().filter(|sem| !sem.is_empty());
        let internal = self.schema.internal();
        let len = usize::from(!meta.is_empty())
            + usize::from(sem.is_some())
            + usize::from(internal.is_some());
        let mut map = serializer.serialize_map(Some(len))?;
        if !meta.is_empty() {
            map.serialize_entry("m", meta)?;
        }
        if let Some(sem) = sem {
            map.serialize_entry("s", sem)?;
        }
        if let Some(internal) = internal {
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
    defs: &'a [&'static Schema],
    internal: &'static Internal,
}

impl Serialize for SchemaChildren<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let len = 2 + usize::from(matches!(self.internal, Internal::Homogeneous(_)));
        let mut map = serializer.serialize_map(Some(len))?;
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

struct NamedChildren<'a> {
    defs: &'a [&'static Schema],
    children: &'static [Named],
}

impl Serialize for NamedChildren<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_map(self.children.iter().map(|child| {
            (
                child.name(),
                ChildRef {
                    defs: self.defs,
                    schema: child.schema(),
                    meta: child.edge_meta(),
                },
            )
        }))
    }
}

struct NumberedChildren<'a> {
    defs: &'a [&'static Schema],
    children: &'static [Numbered],
}

impl Serialize for NumberedChildren<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(self.children.iter().map(|child| ChildRef {
            defs: self.defs,
            schema: child.schema(),
            meta: child.edge_meta(),
        }))
    }
}

struct ChildRef<'a> {
    defs: &'a [&'static Schema],
    schema: &'static Schema,
    meta: &'a Meta,
}

impl Serialize for ChildRef<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let reference = self
            .defs
            .iter()
            .position(|schema| ptr::eq(*schema, self.schema))
            .unwrap();
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
    let end = payload.len().saturating_sub(1); // Reserve the final newline.

    while let Some(definition) = defs.definition(next + count) {
        let written = payload
            .get_mut(len..end)
            .and_then(|buf| serde_json_core::to_slice(&definition, buf).ok());
        let Some(written) = written else {
            if count == 0 {
                return Err(next);
            }
            break;
        };
        len += written;
        payload[len] = b'\n';
        len += 1;
        count += 1;
    }

    Ok(SchemaPage { count, len })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sem, TreeSchema, Ty};

    #[test]
    #[cfg(feature = "json-core")]
    fn page_boundaries() {
        static LEAF: Schema = Schema::LEAF;
        static ROOT: Schema = Schema::named(&[
            Named::new("x", &LEAF, Meta::EMPTY),
            Named::new("y", &LEAF, Meta::EMPTY),
        ]);
        assert_eq!(SchemaDefs::<0>::new(&ROOT).err(), Some(1));
        assert_eq!(SchemaDefs::<1>::new(&ROOT).err(), Some(2));
        let defs = SchemaDefs::<2>::new(&ROOT).unwrap();
        let expected = b"{}\n{\"i\":{\"k\":\"n\",\"c\":{\"x\":0,\"y\":0}}}\n";
        let mut buf = [0; 128];
        for capacity in [expected.len() - 3, expected.len()] {
            let mut actual = std::vec::Vec::new();
            let mut next = 0;
            while next < defs.len() {
                let page = serialize_schema_page(&defs, next, &mut buf[..capacity]).unwrap();
                assert!(page.count > 0);
                assert_eq!(buf[page.len - 1], b'\n');
                actual.extend_from_slice(&buf[..page.len]);
                next += page.count;
            }
            assert_eq!(actual, expected);
        }
        assert_eq!(serialize_schema_page(&defs, 0, &mut []), Err(0));
        assert_eq!(
            serialize_schema_page(&defs, 1, &mut buf[..expected.len() - 4]),
            Err(1)
        );
        assert_eq!(
            serialize_schema_page(&defs, 2, &mut []),
            Ok(SchemaPage { count: 0, len: 0 })
        );
    }

    #[test]
    fn map_lengths() {
        const SCHEMAS: &[&Schema] = &[
            &Schema::LEAF,
            &Schema::leaf(
                Meta::new(&[("unit", "V")]),
                Sem::new(Some(Ty::U8), false, false),
            ),
            <(u8, bool)>::SCHEMA,
            <[u8; 2]>::SCHEMA,
            &Schema::named(&[Named::new("value", &Schema::LEAF, Meta::EMPTY)]),
        ];
        for &schema in SCHEMAS {
            let defs = SchemaDefs::<8>::new(schema).unwrap();
            for id in 0..defs.len() {
                let definition = defs.definition(id).unwrap();
                let json = serde_json::to_value(&definition).unwrap();
                let mut buf = [0; 256];
                let bytes = postcard::to_slice(&definition, &mut buf).unwrap();
                assert_eq!(usize::from(bytes[0]), json.as_object().unwrap().len());
                if let Some(internal) = definition.schema.internal() {
                    let children = SchemaChildren {
                        defs: definition.defs,
                        internal,
                    };
                    let bytes = postcard::to_slice(&children, &mut buf).unwrap();
                    assert_eq!(usize::from(bytes[0]), json["i"].as_object().unwrap().len());
                }
            }
        }
    }
}
