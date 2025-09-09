//! JSON Schema tools

use schemars::{json_schema, JsonSchema, Schema, SchemaGenerator};
use serde_json::Map;
use serde_reflection::{ContainerFormat, Format, Named, VariantFormat};

use crate::{trace::Node, Internal, Meta};

/// Disallow additional items and additional or missing properties
pub fn strictify(schema: &mut Schema) {
    if let Some(o) = schema.as_object_mut() {
        if o.contains_key("prefixItems") {
            debug_assert_eq!(o.insert("items".to_string(), false.into()), None);
        }
        if o.contains_key("items") {
            if let Some(old) = o.insert("type".to_string(), "array".into()) {
                debug_assert_eq!(old, "array");
            }
        }
        if let Some(k) = o.get("properties") {
            let k = k.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
            debug_assert_eq!(o.insert("required".to_string(), k.into()), None);
            debug_assert_eq!(
                o.insert("additionalProperties".to_string(), false.into()),
                None
            );
        }
        if o.contains_key("additionalProperties") {
            if let Some(old) = o.insert("type".to_string(), "object".into()) {
                debug_assert_eq!(old, "object");
            }
        }
    }
}

/// Converted ordered kay-value pairs to properties object
/// Use before `strictify`
pub fn unordered(schema: &mut Schema) {
    if let Some(o) = schema.as_object_mut() {
        if o.remove("x-object") == Some(true.into()) {
            if let Some(t) = o.insert("type".to_string(), "object".into()) {
                debug_assert_eq!(t, "array");
            }
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
            o.insert("properties".to_string(), props.into());
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
            Format::Variable(_variable) => None?, // Unresolved
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
            Format::Option(format) => {
                json_schema!({"oneOf": [format.json_schema(generator)?, {"const": null}]})
            }
            Format::Seq(format) => json_schema!({"items": format.json_schema(generator)?}),
            Format::Map { key, value } => {
                if matches!(**key, Format::Str) {
                    json_schema!({"additionalProperties": value.json_schema(generator)?})
                } else {
                    json_schema!({
                        "items": {
                            "prefixItems": [
                                key.json_schema(generator)?,
                                value.json_schema(generator)?
                            ],
                        }
                    })
                }
            }
            Format::Tuple(formats) => formats.json_schema(generator)?,
            Format::TupleArray { content, size } => json_schema!({
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
            .map(|n| Some(json_schema!({"properties": {&n.name: n.value.json_schema(generator)?}})))
            .collect();
        Some(json_schema!({
            "x-object": true, // Allow transform to unordered object
            "prefixItems": items?,
        }))
    }
}

impl ReflectJsonSchema for Vec<Format> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        let items: Option<Vec<_>> = self.iter().map(|f| f.json_schema(generator)).collect();
        Some(json_schema!({"prefixItems": items?}))
    }
}

impl ReflectJsonSchema for ContainerFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        match self {
            ContainerFormat::UnitStruct => Some(<()>::json_schema(generator)),
            ContainerFormat::NewTypeStruct(format) => format.json_schema(generator),
            ContainerFormat::TupleStruct(formats) => formats.json_schema(generator),
            ContainerFormat::Struct(nameds) => nameds.json_schema(generator),
            ContainerFormat::Enum(map) => {
                let variants: Option<Vec<_>> = map
                    .values()
                    .map(|n| {
                        let mut sch = n.value.json_schema(generator)?;
                        Some(if sch.as_bool() == Some(false) {
                            // Unit variant
                            json_schema!({"const": &n.name})
                        } else if generator.settings().untagged_enum_variant_titles {
                            sch.insert("title".to_string(), n.name.clone().into());
                            sch
                        } else {
                            json_schema!({"properties": {&n.name: sch}})
                        })
                    })
                    .collect();
                variants.map(|v| json_schema!({"oneOf": v}))
            }
        }
    }
}

impl ReflectJsonSchema for VariantFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        match self {
            VariantFormat::Variable(_variable) => None,
            VariantFormat::Unit => Some(false.into()), // Hack picked up in ContainerFormat as ReflectJsonSchema
            VariantFormat::NewType(format) => format.json_schema(generator),
            VariantFormat::Tuple(formats) => formats.json_schema(generator),
            VariantFormat::Struct(nameds) => nameds.json_schema(generator),
        }
    }
}

impl ReflectJsonSchema for Node<(&'static crate::Schema, Option<Format>)> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<Schema> {
        let mut sch = if let Some(internal) = self.data.0.internal.as_ref() {
            match internal {
                Internal::Named(nameds) => {
                    let items: Option<Vec<_>> = nameds
                        .iter()
                        .zip(&self.children)
                        .map(|(named, child)| {
                            let mut sch = child.json_schema(generator)?;
                            push_meta(&mut sch, "x-outer", &named.meta);
                            Some(json_schema!({"properties": {*named.name: sch}}))
                        })
                        .collect();
                    json_schema!({
                        "x-object": true, // Allow transform to unordered object
                        "prefixItems": items?,
                    })
                }
                Internal::Numbered(numbereds) => {
                    let items: Option<Vec<_>> = numbereds
                        .iter()
                        .zip(&self.children)
                        .map(|(numbered, child)| {
                            let mut sch = child.json_schema(generator)?;
                            push_meta(&mut sch, "x-outer", &numbered.meta);
                            Some(sch)
                        })
                        .collect();
                    json_schema!({"prefixItems": items?})
                }
                Internal::Homogeneous(homogeneous) => {
                    let mut sch = json_schema!({
                        "items": self.children[0].json_schema(generator)?,
                        "minItems": homogeneous.len,
                        "maxItems": homogeneous.len
                    });
                    push_meta(&mut sch, "x-outer", &homogeneous.meta);
                    sch
                }
            }
        } else {
            let mut sch = self.data.1.as_ref()?.json_schema(generator)?;
            sch.insert("x-leaf".to_string(), true.into());
            sch
        };
        push_meta(&mut sch, "x-inner", &self.data.0.meta);
        #[cfg(feature = "meta-str")]
        if let Some(meta) = self.data.0.meta {
            if let Some((_, name)) = meta.iter().find(|(key, _)| *key == "name") {
                let name = format!("x-internal-{name}");
                if let Some(existing) = generator.definitions().get(&name) {
                    assert_eq!(existing, sch.as_value());
                } else {
                    generator
                        .definitions_mut()
                        .insert(name.to_string(), sch.into());
                }
                return Some(Schema::new_ref(format!("#/$defs/{name}")));
            }
        }
        Some(sch)
    }
}

#[allow(unused_variables)]
fn push_meta(sch: &mut Schema, key: &str, meta: &Option<Meta>) {
    #[cfg(feature = "meta-str")]
    if let Some(meta) = meta {
        sch.insert(
            key.to_string(),
            meta.iter()
                .map(|(k, v)| (k.to_string(), v.to_string().into()))
                .collect::<Map<_, _>>()
                .into(),
        );
    }
}
