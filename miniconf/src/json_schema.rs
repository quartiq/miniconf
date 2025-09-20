//! JSON Schema tools

use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings, json_schema};
use serde_json::{Map, Value};
use serde_reflection::{
    ContainerFormat, Format, Named, Samples, Tracer, TracerConfig, VariantFormat,
};

use crate::{
    Internal, Meta, TreeDeserialize, TreeSerialize,
    trace::{Node, Types},
};

/// Disallow additional items and additional or missing properties
pub fn strictify(schema: &mut schemars::Schema) {
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

/// Allow null for a x-internal
pub fn internal_absent(schema: &mut schemars::Schema) {
    if let Some(o) = schema.as_object_mut() {
        if o.get("x-leaf") == Some(&true.into()) {
            let v = o.get_mut("type").unwrap();
            let Value::String(t) = v else { panic!() };
            *v = Value::Array(vec![t.clone().into(), "null".into()]);
        }
    }
}

/// Capability to convert serde-reflect formats and graph::Node to to JSON schemata
pub trait ReflectJsonSchema {
    /// Convert to JSON schema
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema>;
}

impl ReflectJsonSchema for Format {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        Some(match self {
            Format::Variable(_variable) => None?, // Unresolved
            Format::TypeName(name) => schemars::Schema::new_ref(format!("#/$defs/{name}")),
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
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        let items: Option<Map<_, _>> = self
            .iter()
            .map(|n| Some((n.name.to_string(), n.value.json_schema(generator)?.into())))
            .collect();
        Some(json_schema!({
            "properties": items?,
        }))
    }
}

impl ReflectJsonSchema for Vec<Format> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        let items: Option<Vec<_>> = self.iter().map(|f| f.json_schema(generator)).collect();
        Some(json_schema!({"prefixItems": items?}))
    }
}

impl ReflectJsonSchema for ContainerFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
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
                Some(json_schema!({"oneOf": variants?}))
            }
        }
    }
}

impl ReflectJsonSchema for VariantFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
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
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        let mut sch = if let Some(internal) = self.data.0.internal.as_ref() {
            match internal {
                Internal::Named(nameds) => {
                    let items: Option<Map<_, _>> = nameds
                        .iter()
                        .zip(&self.children)
                        .map(|(named, child)| {
                            let mut sch = child.json_schema(generator)?;
                            push_meta(&mut sch, "x-outer", &named.meta);
                            Some((named.name.to_string(), sch.into()))
                        })
                        .collect();
                    json_schema!({
                        "properties": items?,
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
        if let Some(meta) = self.data.0.meta {
            #[cfg(feature = "meta-str")]
            if let Some((_, typename)) = meta.iter().find(|(key, _)| *key == "typename") {
                // Convert to named def reference
                let name = format!("x-internal-{typename}");
                if let Some(existing) = generator.definitions().get(&name) {
                    assert_eq!(existing, sch.as_value()); // typename not unique
                } else {
                    generator
                        .definitions_mut()
                        .insert(name.to_string(), sch.into());
                }
                return Some(schemars::Schema::new_ref(format!("#/$defs/{name}")));
            }
        }
        Some(sch)
    }
}

fn push_meta(sch: &mut schemars::Schema, key: &str, meta: &Option<Meta>) {
    if let Some(meta) = meta {
        #[cfg(feature = "meta-str")]
        sch.insert(
            key.to_string(),
            meta.iter()
                .map(|(k, v)| (k.to_string(), v.to_string().into()))
                .collect::<Map<_, _>>()
                .into(),
        );
        #[cfg(not(any(feature = "meta-str")))]
        let _ = (sch, meta, key);
    }
}

/// A JSON Schema and byproducts built from a Tree
pub struct TreeJsonSchema<T> {
    /// Schemata and format tree
    pub types: Types<T>,
    /// Type registry built by tracing
    pub registry: serde_reflection::Registry,
    /// JSON schema generator used
    pub generator: schemars::SchemaGenerator,
    /// Root JSON schema
    pub root: schemars::Schema,
}

impl<'de, T: TreeSerialize + TreeDeserialize<'de> + Default> TreeJsonSchema<T> {
    /// Convert a Tree into a JSON Schema
    pub fn new() -> Result<Self, serde_reflection::Error> {
        let mut types: Types<T> = Default::default();
        let mut tracer = Tracer::new(TracerConfig::default().is_human_readable(true));

        let mut samples = Samples::new();

        // Trace using TreeSerialize
        // If the value does not contain a value for a leaf node (e.g. KeyError::Absent),
        // it will leave the leaf node format unresolved.
        let value = T::default();
        types.trace_values(&mut tracer, &mut samples, &value)?;

        // Trace using TreeDeserialize assuming no samples are needed
        // If the Deserialize can't conjure up a value, it will leave the leaf node format unresolved.
        types.trace_types_simple(&mut tracer)?;

        let registry = tracer.registry()?;

        let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
        let defs: Vec<_> = registry
            .iter()
            .map(|(name, value)| (name.clone(), value.json_schema(&mut generator).into()))
            .collect();
        generator.definitions_mut().extend(defs);

        types.normalize()?;
        let mut root = types.root().json_schema(&mut generator).ok_or(
            serde_reflection::Error::UnknownFormatInContainer("reflection incomplete".to_string()),
        )?;
        root.insert("$defs".to_string(), generator.definitions().clone().into());
        if let Some(meta_schema) = generator.settings().meta_schema.as_deref() {
            root.insert("$schema".to_string(), meta_schema.into());
        }
        Ok(Self {
            types,
            registry,
            generator,
            root,
        })
    }
}
