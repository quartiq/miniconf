//! JSON Schema tools

use core::convert::Infallible;
use std::collections::BTreeMap;

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde_json::Map;
use serde_reflection::{ContainerFormat, Format, Named, VariantFormat};

use crate::{trace::Types, Internal, Meta, Packed, Path, TreeKey};

/// Disallow additional items and additional or missing properties
pub fn strictify(schema: &mut Schema) {
    if let Some(o) = schema.as_object_mut() {
        if o.contains_key("prefixItems") {
            debug_assert_eq!(o.insert("items".into(), false.into()), None);
        }
        if let Some(k) = o
            .get("properties")
            .map(|p| p.as_object().unwrap().keys().cloned().collect::<Vec<_>>())
        {
            debug_assert_eq!(o.insert("additionalProperties".into(), false.into()), None);
            debug_assert_eq!(o.insert("required".into(), k.into()), None);
        }
    }
}

/// Converted ordered kay-value pairs to properties object
/// Use before `strictify`
pub fn unordered(schema: &mut Schema) {
    if let Some(o) = schema.as_object_mut() {
        if o.remove("x-tree-ordered-object") == Some(true.into()) {
            let t = o.get_mut("type").unwrap();
            debug_assert_eq!(t, "array");
            *t = "object".into();
            let props: Map<_, _> = o
                .remove("prefixItems")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    v.as_object()
                        .unwrap()
                        .get("properties")
                        .unwrap()
                        .as_object()
                        .unwrap()
                        .clone()
                        .into_iter()
                        .next()
                        .unwrap()
                })
                .collect();
            o.insert("properties".into(), props.into());
        }
    }
}

/// Capability to convert serde-reflect formats and graph::Node to to JSON schemata
pub trait ReflectJsonSchema {
    /// Convert to JSON schema
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema>;
}

impl ReflectJsonSchema for Format {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        Some(match self {
            Format::Variable(_variable) => None?,
            Format::TypeName(name) => Schema::new_ref(format!("#/$defs/{name}")),
            Format::Unit => <()>::json_schema(generator),
            Format::Bool => bool::json_schema(generator),
            Format::I8 => i8::json_schema(generator),
            Format::I16 => i16::json_schema(generator),
            Format::I32 => i32::json_schema(generator),
            Format::I64 => i64::json_schema(generator),
            Format::I128 => i128::json_schema(generator),
            Format::U8 => u8::json_schema(generator),
            Format::U16 => u16::json_schema(generator),
            Format::U32 => u32::json_schema(generator),
            Format::U64 => u64::json_schema(generator),
            Format::U128 => u128::json_schema(generator),
            Format::F32 => f32::json_schema(generator),
            Format::F64 => f64::json_schema(generator),
            Format::Char => char::json_schema(generator),
            Format::Str => str::json_schema(generator),
            Format::Bytes => <[u8]>::json_schema(generator),
            Format::Option(format) => json_schema!({
                "oneOf": [
                    format.json_schema(generator)?,
                    {"const": null}
                ]
            }),
            Format::Seq(format) => json_schema!({
                "type": "array",
                "items": format.json_schema(generator)?
            }),
            Format::Map { key, value } => json_schema!({
                "type": "array", // keys may not be str
                "items": {
                    "type": "array",
                    "prefixItems": [
                        key.json_schema(generator)?,
                        value.json_schema(generator)?
                    ],
                }
            }),
            Format::Tuple(formats) => formats.json_schema(generator)?,
            Format::TupleArray { content, size } => json_schema!({
                "type": "array",
                "items": content.json_schema(generator)?,
                "minItems": size,
                "maxItems": size
            }),
        })
    }
}

impl ReflectJsonSchema for Vec<Named<Format>> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        let items: Option<Vec<_>> = self
            .iter()
            .map(|n| {
                n.value.json_schema(generator).map(|s| {
                    json_schema!({
                        "type": "object",
                        "properties": {
                            &n.name: s
                        },
                    })
                })
            })
            .collect();
        Some(json_schema!({
            "type": "array",
            "x-tree-ordered-object": true, // Allow transform to unordered object
            "prefixItems": items?,
        }))
    }
}

impl ReflectJsonSchema for Vec<Format> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        let items: Option<Vec<_>> = self.iter().map(|f| f.json_schema(generator)).collect();
        Some(json_schema!({
            "type": "array",
            "prefixItems": items?,
        }))
    }
}

impl ReflectJsonSchema for ContainerFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        match self {
            ContainerFormat::UnitStruct => Some(<()>::json_schema(generator)), // TODO
            ContainerFormat::NewTypeStruct(format) => format.json_schema(generator),
            ContainerFormat::TupleStruct(formats) => formats.json_schema(generator),
            ContainerFormat::Struct(nameds) => nameds.json_schema(generator),
            ContainerFormat::Enum(map) => {
                let variants: Option<Vec<_>> = map
                    .values()
                    .map(|n| {
                        n.value.json_schema(generator).map(|mut s| {
                            if s.as_bool() == Some(false) {
                                // Unit variant
                                json_schema!({
                                    "const": &n.name,
                                })
                            } else {
                                if generator.settings().untagged_enum_variant_titles {
                                    s.insert("title".into(), n.name.clone().into());
                                    s
                                } else {
                                    json_schema!({
                                        "type": "object",
                                        "properties": {
                                            &n.name: s
                                        },
                                    })
                                }
                            }
                        })
                    })
                    .collect();
                variants.map(|v| {
                    json_schema!({
                        "oneOf": v,
                    })
                })
            }
        }
    }
}

impl ReflectJsonSchema for VariantFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        match self {
            VariantFormat::Variable(_variable) => None,
            VariantFormat::Unit => Some(false.into()), // FIXME
            VariantFormat::NewType(format) => format.json_schema(generator),
            VariantFormat::Tuple(formats) => formats.json_schema(generator),
            VariantFormat::Struct(nameds) => nameds.json_schema(generator),
        }
    }
}

fn build_schema(
    idx: &mut [usize],
    depth: usize,
    root: &crate::Schema,
    schema: &crate::Schema,
    generator: &mut SchemaGenerator,
    leaves: &BTreeMap<Packed, Option<Format>>,
) -> Option<Schema> {
    let mut sch = match schema.internal.as_ref() {
        Some(Internal::Named(nameds)) => {
            // No "object" to keep ordering.
            let items: Option<Vec<_>> = nameds
                .iter()
                .enumerate()
                .map(|(i, named)| {
                    idx[depth] = i;
                    build_schema(idx, depth + 1, root, named.schema, generator, leaves).map(
                        |mut sch| {
                            push_meta(&mut sch, "x-tree-outer", &named.meta);
                            json_schema!({
                                "type": "object",
                                "properties": {
                                    *named.name: sch
                                },
                            })
                        },
                    )
                })
                .collect();
            json_schema!({
                "type": "array",
                "x-tree-ordered-object": true, // Allow transform to unordered object
                "prefixItems": items?,
            })
        }
        Some(crate::Internal::Numbered(numbereds)) => {
            let items: Option<Vec<_>> = numbereds
                .iter()
                .enumerate()
                .map(|(i, numbered)| {
                    idx[depth] = i;
                    build_schema(idx, depth + 1, root, numbered.schema, generator, leaves).map(
                        |mut sch| {
                            push_meta(&mut sch, "x-tree-outer", &numbered.meta);
                            sch
                        },
                    )
                })
                .collect();
            json_schema!({
                    "type": "array",
                    "prefixItems": items?,
            })
        }
        Some(crate::Internal::Homogeneous(homogeneous)) => {
            idx[depth] = 0;
            let child = build_schema(idx, depth + 1, root, homogeneous.schema, generator, leaves)?;
            let mut sch = json_schema!({
                "type": "array",
                "items": child,
                "minItems": homogeneous.len,
                "maxItems": homogeneous.len
            });
            push_meta(&mut sch, "x-tree-outer", &homogeneous.meta);
            sch
        }
        None => {
            let p: Packed = root.transcode(&idx[..depth]).unwrap();
            let mut sch = leaves.get(&p)?.as_ref()?.json_schema(generator)?;
            sch.insert("x-tree-leaf".into(), true.into());
            sch
        }
    };
    push_meta(&mut sch, "x-tree-inner", &schema.meta);
    Some(sch)
}

fn push_meta(sch: &mut Schema, key: &str, meta: &Meta) {
    if let Some(meta) = meta {
        sch.insert(
            key.to_string(),
            meta.iter()
                .map(|(k, v)| [k.to_string(), v.to_string()])
                .collect::<Vec<_>>()
                .into(),
        );
    }
}

impl crate::Schema {
    pub fn visit<T, E>(
        &self,
        idx: &mut [usize],
        depth: usize,
        func: &mut impl FnMut(&[usize], &Meta, Option<(&Internal, Vec<T>)>) -> Result<T, E>,
    ) -> Result<T, E> {
        if let Some(internal) = self.internal.as_ref() {
            let children: Result<Vec<_>, _> = match internal {
                Internal::Homogeneous(h) => {
                    idx[depth] = 0;
                    [h.schema.visit(idx, depth + 1, func)].into_iter().collect()
                }
                Internal::Named(n) => n
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        idx[depth] = i;
                        n.schema.visit(idx, depth + 1, func)
                    })
                    .collect(),
                Internal::Numbered(n) => n
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        idx[depth] = i;
                        n.schema.visit(idx, depth + 1, func)
                    })
                    .collect(),
            };
            func(&idx[..depth], &self.meta, Some((internal, children?)))
        } else {
            func(&idx[..depth], &self.meta, None)
        }
    }
}

impl<T: TreeKey> ReflectJsonSchema for Types<T, Format> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        let mut idx = vec![0; T::SCHEMA.shape().max_depth];
        T::SCHEMA
            .visit(&mut idx, 0, &mut |idx, meta, internal_children| {
                let mut sch = if let Some((internal, children)) = internal_children {
                    match internal {
                        Internal::Named(nameds) => todo!(),
                        Internal::Numbered(numbereds) => todo!(),
                        Internal::Homogeneous(homogeneous) => todo!(),
                    }
                } else {
                    let p: Path<String, '/'> = T::SCHEMA.transcode(idx).unwrap();
                    let mut sch = self
                        .leaves
                        .get(&p.into_inner())
                        .unwrap()
                        .as_ref()
                        .and_then(|s| s.json_schema(generator))
                        .ok_or(())?;
                    sch.insert("x-tree-leaf".into(), true.into());
                    sch
                };
                push_meta(&mut sch, "x-tree-inner", meta);
                Ok::<_, ()>(Some(sch))
            })
            .unwrap()
    }
}

// TODO:
// unit variant/struct: json_schema!({"type": "string", "const": name})
