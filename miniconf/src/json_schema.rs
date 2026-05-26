//! JSON Schema tools.
//!
//! The generated schemas use a small set of `miniconf`-specific extension keys in addition to
//! standard JSON Schema:
//!
//! - `tree-leaf`: this schema node is a `miniconf` leaf
//! - `tree-node-meta`: metadata attached to the addressed node itself
//! - `tree-edge-meta`: metadata attached to the parent-child edge
//! - `tree-maybe-absent`: this node may serialize as [`TREE_ABSENT`]
//!
//! [`AllowAbsent`] lowers `tree-maybe-absent` into plain JSON Schema `oneOf` forms for validators
//! that do not understand the compact extension key.

use schemars::{
    JsonSchema, SchemaGenerator, generate::SchemaSettings, json_schema, transform::Transform,
};
use serde_json::{Map, Value};
use serde_reflection::{
    ContainerFormat, Format, Named, Samples, Tracer, TracerConfig, VariantFormat,
};

use crate::{
    Internal, Meta, Named as SchemaNamed, Schema, Sem, TreeDeserializeOwned, TreeSerialize,
    json::{self, TREE_ABSENT},
    trace::{Node, Types},
};

const TREE_LEAF: &str = "tree-leaf";
const TREE_NODE_META: &str = "tree-node-meta";
const TREE_EDGE_META: &str = "tree-edge-meta";
const TREE_MAYBE_ABSENT: &str = "tree-maybe-absent";
const META_NULLABLE: &str = "nullable";
const META_TYPENAME: &str = "typename";

/// Allow [`TREE_ABSENT`] nodes by lowering `tree-maybe-absent` to `oneOf`.
pub struct AllowAbsent;
impl Transform for AllowAbsent {
    fn transform(&mut self, schema: &mut schemars::Schema) {
        if let Some(o) = schema.as_object_mut()
            && o.get(TREE_MAYBE_ABSENT) == Some(&true.into())
        {
            o.remove(TREE_MAYBE_ABSENT).unwrap();
            *schema = json_schema!({"oneOf": [schema, {"const": TREE_ABSENT}]});
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
            json_schema!({"oneOf": [
                format_schema(format, generator)?,
                {"const": null}
            ]})
        }
        Format::Seq(format) => json_schema!({"items": format_schema(format, generator)?}),
        Format::Map { key, value } => {
            if matches!(**key, Format::Str) {
                json_schema!({
                    "type": "object",
                    "additionalProperties": format_schema(value, generator)?
                })
            } else {
                json_schema!({
                    "type": "array",
                    "items": {
                        "prefixItems": [
                            format_schema(key, generator)?,
                            format_schema(value, generator)?
                        ],
                        "items": false
                    }
                })
            }
        }
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

fn strict_object(
    properties: Map<String, serde_json::Value>,
    required: Vec<&str>,
) -> schemars::Schema {
    json_schema!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn strict_named_variant(
    name: &'static str,
    schema: schemars::Schema,
    required: bool,
) -> schemars::Schema {
    let mut variant = strict_object(
        [(name.to_string(), schema.into())].into_iter().collect(),
        Vec::new(),
    );
    if required {
        variant.insert("required".to_string(), vec![name].into());
    }
    variant
}

fn maybe_absent(node: &TraceNode) -> bool {
    node.data.0.sem().is_some_and(Sem::maybe_absent)
}

fn value_allows_null(value: &Value) -> bool {
    if let Value::Object(object) = value {
        object.get("const") == Some(&Value::Null)
            || object.get("type") == Some(&Value::String("null".to_string()))
            || object
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|types| types.iter().any(|ty| ty == "null"))
            || object
                .get("oneOf")
                .and_then(Value::as_array)
                .is_some_and(|schemas| schemas.iter().any(value_allows_null))
            || object
                .get("anyOf")
                .and_then(Value::as_array)
                .is_some_and(|schemas| schemas.iter().any(value_allows_null))
    } else {
        false
    }
}

fn nullable_schema(mut schema: schemars::Schema) -> schemars::Schema {
    if value_allows_null(schema.as_value()) {
        return schema;
    }
    let tree_leaf = schema.remove(TREE_LEAF);
    let tree_node_meta = schema.remove(TREE_NODE_META);
    let mut wrapper = json_schema!({"oneOf": [schema, {"const": null}]});
    if let Some(tree_leaf) = tree_leaf {
        wrapper.insert(TREE_LEAF.to_string(), tree_leaf);
    }
    if let Some(tree_node_meta) = tree_node_meta {
        wrapper.insert(TREE_NODE_META.to_string(), tree_node_meta);
    }
    wrapper
}

fn definition_name(meta: &Meta) -> Option<String> {
    meta.items.iter().find_map(|(key, typename)| {
        (*key == META_TYPENAME).then_some(format!("tree-internal-{typename}"))
    })
}

struct TreeProjector<'a> {
    generator: &'a mut SchemaGenerator,
}

impl TreeProjector<'_> {
    // Project the Miniconf tree first, then apply node decorations in one place.
    fn node(&mut self, node: &TraceNode, sample: Option<&Value>) -> Option<schemars::Schema> {
        let schema = if let Some(internal) = node.data.0.internal() {
            self.internal(node, internal, sample)?
        } else {
            format_schema(node.data.1.as_ref()?, self.generator)?
        };
        Some(self.finish_node(node, schema))
    }

    fn internal(
        &mut self,
        node: &TraceNode,
        internal: &Internal,
        sample: Option<&Value>,
    ) -> Option<schemars::Schema> {
        Some(match internal {
            Internal::Named(nameds) => {
                let sample = sample.and_then(Value::as_object);
                if node.data.0.sem().is_some_and(Sem::oneof)
                    && node
                        .children
                        .iter()
                        .filter(|child| maybe_absent(child))
                        .count()
                        <= 1
                {
                    self.named_oneof(nameds, &node.children, sample)?
                } else {
                    self.named_object(nameds, &node.children, sample)?
                }
            }
            Internal::Numbered(numbereds) => {
                let sample = sample.and_then(Value::as_array);
                let items: Option<Vec<_>> = numbereds
                    .iter()
                    .zip(&node.children)
                    .enumerate()
                    .map(|(index, (numbered, child))| {
                        self.edge_child(
                            child,
                            sample.and_then(|sample| sample.get(index)),
                            numbered.edge_meta(),
                        )
                    })
                    .collect();
                json_schema!({
                    "type": "array",
                    "prefixItems": items?,
                    "items": false
                })
            }
            Internal::Homogeneous(homogeneous) => {
                let sample = sample
                    .and_then(Value::as_array)
                    .and_then(|sample| sample.first());
                let sch = self.edge_child(&node.children[0], sample, homogeneous.edge_meta())?;
                json_schema!({
                    "type": "array",
                    "items": sch,
                    "minItems": homogeneous.len(),
                    "maxItems": homogeneous.len()
                })
            }
        })
    }

    fn named_oneof(
        &mut self,
        nameds: &[SchemaNamed],
        children: &[TraceNode],
        sample: Option<&Map<String, Value>>,
    ) -> Option<schemars::Schema> {
        let variants: Option<Vec<_>> = nameds
            .iter()
            .zip(children)
            .map(|(named, child)| {
                let sch = self.edge_child(
                    child,
                    sample.and_then(|sample| sample.get(named.name())),
                    named.edge_meta(),
                )?;
                Some(strict_named_variant(
                    named.name(),
                    sch,
                    !maybe_absent(child),
                ))
            })
            .collect();
        Some(json_schema!({"oneOf": variants?}))
    }

    fn named_object(
        &mut self,
        nameds: &[SchemaNamed],
        children: &[TraceNode],
        sample: Option<&Map<String, Value>>,
    ) -> Option<schemars::Schema> {
        let mut required = Vec::new();
        let items: Option<Map<_, _>> = nameds
            .iter()
            .zip(children)
            .map(|(named, child)| {
                let sch = self.edge_child(
                    child,
                    sample.and_then(|sample| sample.get(named.name())),
                    named.edge_meta(),
                )?;
                if required_named_child(sample, child, named.name()) {
                    required.push(named.name());
                }
                Some((named.name().to_string(), sch.into()))
            })
            .collect();
        Some(strict_object(items?, required))
    }

    fn edge_child(
        &mut self,
        child: &TraceNode,
        sample: Option<&Value>,
        edge_meta: &Meta,
    ) -> Option<schemars::Schema> {
        let mut schema = self.node(child, sample)?;
        if edge_meta.get(META_NULLABLE) == Some("true") {
            schema = nullable_schema(schema);
        }
        push_meta(&mut schema, TREE_EDGE_META, edge_meta);
        Some(schema)
    }

    fn finish_node(&mut self, node: &TraceNode, mut schema: schemars::Schema) -> schemars::Schema {
        let maybe_absent = maybe_absent(node);
        let is_leaf = node.data.0.internal().is_none();
        push_tree_leaf(&mut schema, is_leaf);
        push_meta(&mut schema, TREE_NODE_META, node.data.0.node_meta());

        if let Some(name) = definition_name(node.data.0.node_meta()) {
            return self.finish_reference(node, schema, name, maybe_absent);
        }

        if node.data.0.node_meta().get(META_NULLABLE) == Some("true") {
            schema = nullable_schema(schema);
        }
        if maybe_absent {
            schema.insert(TREE_MAYBE_ABSENT.to_string(), true.into());
        }
        schema
    }

    fn finish_reference(
        &mut self,
        node: &TraceNode,
        schema: schemars::Schema,
        name: String,
        maybe_absent: bool,
    ) -> schemars::Schema {
        let def = schema.clone();
        if let Some(existing) = self.generator.definitions().get(&name) {
            assert_eq!(existing, def.as_value()); // typename not unique
        } else {
            self.generator
                .definitions_mut()
                .insert(name.to_string(), def.into());
        }
        let mut reference = schemars::Schema::new_ref(format!("#/$defs/{name}"));
        push_tree_leaf(&mut reference, node.data.0.internal().is_none());
        if node.data.0.node_meta().get(META_NULLABLE) == Some("true") {
            reference = nullable_schema(reference);
        }
        if maybe_absent {
            reference.insert(TREE_MAYBE_ABSENT.to_string(), true.into());
        }
        reference
    }
}

fn required_named_child(
    sample: Option<&Map<String, Value>>,
    child: &TraceNode,
    name: &'static str,
) -> bool {
    if let Some(sample) = sample {
        sample.contains_key(name)
    } else {
        !maybe_absent(child)
    }
}

fn push_meta(sch: &mut schemars::Schema, key: &str, meta: &Meta) {
    if !meta.is_empty() {
        assert_eq!(
            sch.insert(
                key.to_string(),
                meta.items
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string().into()))
                    .collect::<Map<_, _>>()
                    .into(),
            ),
            None
        );
    }
}

fn push_tree_leaf(sch: &mut schemars::Schema, leaf: bool) {
    if leaf {
        assert_eq!(sch.insert(TREE_LEAF.to_string(), true.into()), None);
    }
}

/// A JSON Schema and byproducts built from a Tree
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
    pub fn new(value: Option<&T>) -> Result<Self, serde_reflection::Error> {
        let sample = value
            .map(json::to_json_value)
            .transpose()
            .map_err(|e| serde_reflection::Error::Custom(e.to_string()))?;
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
        }
        .node(types.root(), sample.as_ref())
        .ok_or(serde_reflection::Error::UnknownFormatInContainer(
            "reflection incomplete".to_string(),
        ))?;
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
