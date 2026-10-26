//! JSON Schema tools.
//!
//! The generated schemas use a small set of `miniconf`-specific extension keys in addition to
//! standard JSON Schema:
//!
//! - `tree-kind`: `leaf`, `named`, `numbered`, or `homogeneous`
//! - `tree-oneof`: a named node has mutually exclusive children
//! - `tree-node-meta`: metadata attached to the addressed node itself
//! - `tree-edge-meta`: metadata attached to the parent-child edge
//! - `tree-maybe-absent`: this node may serialize as [`TREE_ABSENT`]
//!
//! Tree definitions live in `$defs`; the root and child uses reference them.
//! Definition names are local to an export, not persistent type identifiers.
//! Equal Miniconf schemata and serialized representations share definitions.
//! This is structural equivalence, including metadata, not Rust type identity.
//! Node metadata belongs to definitions, edge metadata to uses. Nullable uses
//! have the canonical shape `anyOf: [reference, {type: "null"}]`.
//! Named children always live in `properties`, including mutually exclusive
//! children. Requiredness follows schema semantics, not the supplied sample.
//!
//! [`AllowAbsent`] lowers `tree-maybe-absent` into plain JSON Schema `anyOf` forms for validators
//! that do not understand the compact extension key.
//!
//! Host reference generators should consume this projection before `AllowAbsent`.
//! Applications may attach a root `default` from [`crate::json::to_json_value`];
//! its reserved diagnostic strings represent node availability, separately from
//! JSON null. Traced leaf representations describe the discovered serialization
//! shape; applications must supply enough samples for otherwise unobserved variants.

use schemars::{
    JsonSchema, SchemaGenerator, generate::SchemaSettings, json_schema, transform::Transform,
};
use serde_json::Map;
use serde_reflection::{
    ContainerFormat, Error, Format, Named, Samples, Tracer, TracerConfig, VariantFormat,
};

use crate::{
    Internal, Meta, Schema, Sem, TreeDeserializeOwned, TreeSerialize,
    json::TREE_ABSENT,
    trace::{Node, Types},
};

const TREE_NODE_META: &str = "tree-node-meta";
const TREE_EDGE_META: &str = "tree-edge-meta";
const TREE_MAYBE_ABSENT: &str = "tree-maybe-absent";
const META_NULLABLE: &str = "nullable";

/// Allow [`TREE_ABSENT`] nodes by lowering `tree-maybe-absent` to `anyOf`.
pub struct AllowAbsent;
impl Transform for AllowAbsent {
    fn transform(&mut self, schema: &mut schemars::Schema) {
        if let Some(o) = schema.as_object_mut()
            && o.get(TREE_MAYBE_ABSENT) == Some(&true.into())
        {
            o.remove(TREE_MAYBE_ABSENT).unwrap();
            *schema = json_schema!({"anyOf": [schema, {"const": TREE_ABSENT}]});
        }
        schemars::transform::transform_subschemas(self, schema);
    }
}

fn format_schema(format: &Format, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
    Some(match format {
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
            json_schema!({"anyOf": [
                format_schema(format, generator)?,
                {"const": null}
            ]})
        }
        Format::Seq(format) => json_schema!({
            "type": "array", "items": format_schema(format, generator)?
        }),
        // serde_json encodes supported map keys as object property names,
        // including integer keys. Unsupported keys fail serialization.
        Format::Map { value, .. } => json_schema!({
            "type": "object", "additionalProperties": format_schema(value, generator)?
        }),
        Format::Tuple(formats) => tuple_schema(formats, generator)?,
        Format::TupleArray { content, size } => json_schema!({
            "type": "array",
            "items": format_schema(content, generator)?,
            "minItems": size,
            "maxItems": size
        }),
    })
}

fn named_fields_schema(
    fields: &[Named<Format>],
    generator: &mut SchemaGenerator,
) -> Option<schemars::Schema> {
    let items: Option<Map<_, _>> = fields
        .iter()
        .map(|n| {
            Some((
                n.name.to_string(),
                format_schema(&n.value, generator)?.into(),
            ))
        })
        .collect();
    let required = fields.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
    Some(json_schema!({
        "type": "object",
        "properties": items?,
        "required": required,
        "additionalProperties": false,
    }))
}

fn tuple_schema(fields: &[Format], generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
    let items: Option<Vec<_>> = fields
        .iter()
        .map(|format| format_schema(format, generator))
        .collect();
    Some(json_schema!({
        "type": "array",
        "prefixItems": items?,
        "minItems": fields.len(),
        "items": false
    }))
}

fn container_schema(
    container: &ContainerFormat,
    generator: &mut SchemaGenerator,
) -> Option<schemars::Schema> {
    match container {
        ContainerFormat::UnitStruct => Some(<()>::json_schema(generator)),
        ContainerFormat::NewTypeStruct(format) => format_schema(format, generator),
        ContainerFormat::TupleStruct(formats) => tuple_schema(formats, generator),
        ContainerFormat::Struct(nameds) => named_fields_schema(nameds, generator),
        ContainerFormat::Enum(map) => {
            let variants: Option<Vec<_>> = map
                .values()
                .map(|n| {
                    let mut schema = variant_schema(&n.value, generator)?;
                    Some(if schema.as_bool() == Some(false) {
                        // Unit variant
                        json_schema!({"const": &n.name})
                    } else if generator.settings().untagged_enum_variant_titles {
                        schema.insert("title".to_string(), n.name.clone().into());
                        schema
                    } else {
                        json_schema!({
                            "type": "object",
                            "properties": {&n.name: schema},
                            "required": [&n.name],
                            "additionalProperties": false
                        })
                    })
                })
                .collect();
            Some(json_schema!({"oneOf": variants?}))
        }
    }
}

fn variant_schema(
    variant: &VariantFormat,
    generator: &mut SchemaGenerator,
) -> Option<schemars::Schema> {
    match variant {
        VariantFormat::Variable(_variable) => None,
        // Serialized as `{variant_name}`. Use the never-match schema to signal this to the enclosing container.
        VariantFormat::Unit => Some(false.into()),
        VariantFormat::NewType(format) => format_schema(format, generator),
        VariantFormat::Tuple(formats) => tuple_schema(formats, generator),
        VariantFormat::Struct(nameds) => named_fields_schema(nameds, generator),
    }
}

type TraceNode = Node<(&'static Schema, Option<Format>)>;

fn maybe_absent(node: &TraceNode) -> bool {
    node.data.0.sem().is_some_and(Sem::maybe_absent)
}

fn push_meta(schema: &mut schemars::Schema, key: &str, meta: &Meta) {
    if !meta.is_empty() {
        schema.insert(
            key.into(),
            meta.iter()
                .map(|(k, v)| (k.to_string(), (*v).into()))
                .collect::<Map<_, _>>()
                .into(),
        );
    }
}

struct TreeProjector<'a> {
    generator: &'a mut SchemaGenerator,
    definitions: Vec<(&'static Schema, schemars::Schema, String)>,
}

impl TreeProjector<'_> {
    fn node(&mut self, node: &TraceNode) -> Result<schemars::Schema, Error> {
        let source = node.data.0;
        let (kind, mut schema) = match source.internal() {
            None => (
                "leaf",
                node.data
                    .1
                    .as_ref()
                    .and_then(|format| format_schema(format, self.generator))
                    .ok_or_else(|| {
                        Error::UnknownFormatInContainer("reflection incomplete".into())
                    })?,
            ),
            Some(Internal::Named(children)) => {
                let properties: Result<Map<_, _>, Error> = children
                    .iter()
                    .zip(&node.children)
                    .map(|(edge, child)| {
                        Ok((
                            edge.name().into(),
                            self.edge(child, edge.edge_meta())?.into(),
                        ))
                    })
                    .collect();
                let oneof = source.sem().is_some_and(Sem::oneof);
                let mut schema = json_schema!({"type": "object", "properties": properties?, "additionalProperties": false});
                if oneof {
                    schema.insert("tree-oneof".into(), true.into());
                    schema.insert("maxProperties".into(), 1.into());
                    if !node.children.iter().any(maybe_absent) {
                        schema.insert("minProperties".into(), 1.into());
                    }
                } else {
                    let required: Vec<_> = children
                        .iter()
                        .zip(&node.children)
                        .filter(|(_, child)| !maybe_absent(child))
                        .map(|(edge, _)| edge.name())
                        .collect();
                    schema.insert("required".into(), required.into());
                }
                ("named", schema)
            }
            Some(Internal::Numbered(children)) => {
                let items: Result<Vec<_>, _> = children
                    .iter()
                    .zip(&node.children)
                    .map(|(edge, child)| self.edge(child, edge.edge_meta()))
                    .collect();
                (
                    "numbered",
                    json_schema!({"type": "array", "prefixItems": items?, "items": false, "minItems": children.len()}),
                )
            }
            Some(Internal::Homogeneous(edge)) => (
                "homogeneous",
                json_schema!({
                    "type": "array", "items": self.edge(&node.children[0], edge.edge_meta())?,
                    "minItems": edge.len(), "maxItems": edge.len()
                }),
            ),
        };
        schema.insert("tree-kind".into(), kind.into());
        push_meta(&mut schema, TREE_NODE_META, source.node_meta());
        if maybe_absent(node) {
            schema.insert(TREE_MAYBE_ABSENT.into(), true.into());
        }
        // Compare schema contents, including child order, rather than addresses
        // of promoted constants. Tracing can supply different leaf formats for
        // otherwise equal Miniconf schemata.
        let reference = if let Some((_, _, reference)) = self
            .definitions
            .iter()
            .find(|(other, value, _)| *other == source && *value == schema)
        {
            reference.clone()
        } else {
            let mut name = format!("tree-{}", self.definitions.len());
            while self.generator.definitions().contains_key(&name) {
                name.push('_');
            }
            self.generator
                .definitions_mut()
                .insert(name.clone(), schema.clone().into());
            let reference = format!("#/$defs/{name}");
            self.definitions.push((source, schema, reference.clone()));
            reference
        };
        Ok(schemars::Schema::new_ref(reference))
    }

    fn edge(&mut self, child: &TraceNode, meta: &Meta) -> Result<schemars::Schema, Error> {
        let mut schema = self.node(child)?;
        if meta.get(META_NULLABLE) == Some("true")
            || child.data.0.node_meta().get(META_NULLABLE) == Some("true")
        {
            schema = json_schema!({"anyOf": [schema, {"type": "null"}]});
        }
        push_meta(&mut schema, TREE_EDGE_META, meta);
        Ok(schema)
    }
}

/// Traced Miniconf tree projected into JSON Schema definitions and references.
pub struct TreeJsonSchema<T> {
    /// Schemata and format tree
    pub types: Types<T>,
    /// Type registry built by tracing
    pub registry: serde_reflection::Registry,
    /// Value samples gathered during tracing
    pub samples: serde_reflection::Samples,
    /// JSON schema generator used
    pub generator: schemars::SchemaGenerator,
    /// Root JSON schema
    pub root: schemars::Schema,
}

impl<T: TreeSerialize + TreeDeserializeOwned> TreeJsonSchema<T> {
    /// Convert a Tree into a JSON Schema
    pub fn new(value: Option<&T>) -> Result<Self, Error> {
        let mut types: Types<T> = Default::default();
        let mut tracer = Tracer::new(
            TracerConfig::default()
                .is_human_readable(true)
                .record_samples_for_newtype_structs(true)
                .record_samples_for_structs(true)
                .record_samples_for_tuple_structs(true),
        );

        let mut samples = Samples::new();

        if let Some(value) = value {
            // Trace using TreeSerialize
            // If the value does not contain a value for a leaf node (e.g. KeyError::Absent),
            // it will leave the leaf node format unresolved.
            types.trace_values(&mut tracer, &mut samples, value)?;
        }

        // Trace using TreeDeserialize assuming no samples are needed
        // If the Deserialize can't conjure up a value, it will leave the leaf node format unresolved.
        // types.trace_types_simple(&mut tracer)?;
        types.trace_types(&mut tracer, &samples)?;

        let registry = tracer.registry()?;

        let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
        let defs: Vec<_> = registry
            .iter()
            .map(|(name, value)| (name.clone(), container_schema(value, &mut generator).into()))
            .collect();
        generator.definitions_mut().extend(defs);

        types.normalize()?;
        let mut root = TreeProjector {
            generator: &mut generator,
            definitions: Vec::new(),
        }
        .edge(types.root(), &Meta::EMPTY)?;
        root.insert("$defs".to_string(), generator.definitions().clone().into());
        if let Some(meta_schema) = generator.settings().meta_schema.as_deref() {
            root.insert("$schema".to_string(), meta_schema.into());
        }
        Ok(Self {
            types,
            samples,
            registry,
            generator,
            root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sharing_compares_schema_contents_and_leaf_formats() {
        static FIRST: Schema = Schema::LEAF;
        static SECOND: Schema = Schema::LEAF;
        assert!(!core::ptr::eq(&FIRST, &SECOND));
        let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
        let mut projector = TreeProjector {
            generator: &mut generator,
            definitions: Vec::new(),
        };
        let references: Vec<_> = [
            (&FIRST, Format::U32),
            (&SECOND, Format::U32),
            (&FIRST, Format::Bool),
        ]
        .into_iter()
        .map(|(schema, format)| {
            projector
                .node(&Node {
                    data: (schema, Some(format)),
                    children: Vec::new(),
                })
                .unwrap()
        })
        .collect();
        assert_eq!(references[0], references[1]);
        assert_ne!(references[0], references[2]);
    }
}
