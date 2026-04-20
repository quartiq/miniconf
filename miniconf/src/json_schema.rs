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
    Internal, Meta, Schema, Sem, TreeDeserializeOwned, TreeSerialize, json,
    trace::{Node, Types},
};

/// Magic JSON Value for absent node values
pub const TREE_ABSENT: &str = "__tree-absent__";
/// Magic JSON Value for access-denied node values
pub const TREE_ACCESS: &str = "__tree-access__";

/// Allow [`TREE_ABSENT`] nodes by lowering `tree-maybe-absent` to `oneOf`.
pub struct AllowAbsent;
impl Transform for AllowAbsent {
    fn transform(&mut self, schema: &mut schemars::Schema) {
        if let Some(o) = schema.as_object_mut()
            && o.get("tree-maybe-absent") == Some(&true.into())
        {
            o.remove("tree-maybe-absent").unwrap();
            *schema = json_schema!({"oneOf": [schema, {"const": TREE_ABSENT}]});
        }
        schemars::transform::transform_subschemas(self, schema);
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
                json_schema!({"oneOf": [
                    format.json_schema(generator)?,
                    {"const": null}
                ]})
            }
            Format::Seq(format) => json_schema!({"items": format.json_schema(generator)?}),
            Format::Map { key, value } => {
                if matches!(**key, Format::Str) {
                    json_schema!({
                        "type": "object",
                        "additionalProperties": value.json_schema(generator)?
                    })
                } else {
                    json_schema!({
                        "type": "array",
                        "items": {
                            "prefixItems": [
                                key.json_schema(generator)?,
                                value.json_schema(generator)?
                            ],
                            "items": false
                        }
                    })
                }
            }
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
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        let items: Option<Map<_, _>> = self
            .iter()
            .map(|n| Some((n.name.to_string(), n.value.json_schema(generator)?.into())))
            .collect();
        let required = self.iter().map(|n| n.name.clone()).collect::<Vec<_>>();
        Some(json_schema!({
            "type": "object",
            "properties": items?,
            "required": required,
            "additionalProperties": false,
        }))
    }
}

impl ReflectJsonSchema for Vec<Format> {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        let items: Option<Vec<_>> = self.iter().map(|f| f.json_schema(generator)).collect();
        Some(json_schema!({
            "type": "array",
            "prefixItems": items?,
            "items": false
        }))
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
                            json_schema!({
                                "type": "object",
                                "properties": {&n.name: sch},
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
}

impl ReflectJsonSchema for VariantFormat {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        match self {
            VariantFormat::Variable(_variable) => None,
            // Serialized as `{variant_name}`. Use the never-match schema to signal this to the enclosing ContainerFormat impl.
            VariantFormat::Unit => Some(false.into()),
            VariantFormat::NewType(format) => format.json_schema(generator),
            VariantFormat::Tuple(formats) => formats.json_schema(generator),
            VariantFormat::Struct(nameds) => nameds.json_schema(generator),
        }
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

fn object_sample(sample: Option<&Value>) -> Option<&Map<String, Value>> {
    match sample {
        Some(Value::Object(sample)) => Some(sample),
        _ => None,
    }
}

fn array_sample(sample: Option<&Value>) -> Option<&Vec<Value>> {
    match sample {
        Some(Value::Array(sample)) => Some(sample),
        _ => None,
    }
}

fn child_sample<'a>(sample: Option<&'a Map<String, Value>>, name: &str) -> Option<&'a Value> {
    sample.and_then(|sample| sample.get(name))
}

fn required_named_child(
    sample: Option<&Map<String, Value>>,
    child: &TraceNode,
    name: &'static str,
) -> bool {
    if let Some(sample) = sample {
        sample.contains_key(name)
    } else {
        !child.data.0.sem().is_some_and(Sem::maybe_absent)
    }
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
    let tree_leaf = schema.remove("tree-leaf");
    let tree_node_meta = schema.remove("tree-node-meta");
    let mut wrapper = json_schema!({"oneOf": [schema, {"const": null}]});
    if let Some(tree_leaf) = tree_leaf {
        wrapper.insert("tree-leaf".to_string(), tree_leaf);
    }
    if let Some(tree_node_meta) = tree_node_meta {
        wrapper.insert("tree-node-meta".to_string(), tree_node_meta);
    }
    wrapper
}

fn definition_name(meta: &Meta) -> Option<String> {
    meta.items.iter().find_map(|(key, typename)| {
        (*key == "typename").then_some(format!("tree-internal-{typename}"))
    })
}

fn node_json_schema(
    node: &TraceNode,
    sample: Option<&Value>,
    generator: &mut SchemaGenerator,
) -> Option<schemars::Schema> {
    let mut sch = if let Some(internal) = node.data.0.internal() {
        match internal {
            Internal::Named(nameds) => {
                let sample = object_sample(sample);
                let maybe_absent = node.children.iter().map(maybe_absent).collect::<Vec<_>>();
                if node.data.0.sem().is_some_and(Sem::oneof)
                    && maybe_absent.iter().filter(|&&child| child).count() <= 1
                {
                    let variants: Option<Vec<_>> = nameds
                        .iter()
                        .zip(&node.children)
                        .zip(&maybe_absent)
                        .map(|((named, child), maybe_absent)| {
                            let mut sch = node_json_schema(
                                child,
                                child_sample(sample, named.name()),
                                generator,
                            )?;
                            if named.edge_meta().get("nullable") == Some("true") {
                                sch = nullable_schema(sch);
                            }
                            push_meta(&mut sch, "tree-edge-meta", named.edge_meta());
                            Some(strict_named_variant(named.name(), sch, !maybe_absent))
                        })
                        .collect();
                    json_schema!({"oneOf": variants?})
                } else {
                    let mut required = Vec::new();
                    let items: Option<Map<_, _>> = nameds
                        .iter()
                        .zip(&node.children)
                        .map(|(named, child)| {
                            let mut sch = node_json_schema(
                                child,
                                child_sample(sample, named.name()),
                                generator,
                            )?;
                            if named.edge_meta().get("nullable") == Some("true") {
                                sch = nullable_schema(sch);
                            }
                            push_meta(&mut sch, "tree-edge-meta", named.edge_meta());
                            if required_named_child(sample, child, named.name()) {
                                required.push(named.name());
                            }
                            Some((named.name().to_string(), sch.into()))
                        })
                        .collect();
                    strict_object(items?, required)
                }
            }
            Internal::Numbered(numbereds) => {
                let sample = array_sample(sample);
                let items: Option<Vec<_>> = numbereds
                    .iter()
                    .zip(&node.children)
                    .enumerate()
                    .map(|(index, (numbered, child))| {
                        let mut sch = node_json_schema(
                            child,
                            sample.and_then(|sample| sample.get(index)),
                            generator,
                        )?;
                        if numbered.edge_meta().get("nullable") == Some("true") {
                            sch = nullable_schema(sch);
                        }
                        push_meta(&mut sch, "tree-edge-meta", numbered.edge_meta());
                        Some(sch)
                    })
                    .collect();
                json_schema!({
                    "type": "array",
                    "prefixItems": items?,
                    "items": false
                })
            }
            Internal::Homogeneous(homogeneous) => {
                let sample = array_sample(sample).and_then(|sample| sample.first());
                let mut sch = node_json_schema(&node.children[0], sample, generator)?;
                if homogeneous.edge_meta().get("nullable") == Some("true") {
                    sch = nullable_schema(sch);
                }
                push_meta(&mut sch, "tree-edge-meta", homogeneous.edge_meta());
                json_schema!({
                    "type": "array",
                    "items": sch,
                    "minItems": homogeneous.len(),
                    "maxItems": homogeneous.len()
                })
            }
        }
    } else {
        node.data.1.as_ref()?.json_schema(generator)?
    };
    let maybe_absent = maybe_absent(node);
    push_tree_leaf(&mut sch, node.data.0.internal().is_none());
    push_meta(&mut sch, "tree-node-meta", node.data.0.node_meta());
    if let Some(name) = definition_name(node.data.0.node_meta()) {
        let mut def = sch.clone();
        if maybe_absent {
            def.remove("tree-maybe-absent");
        }
        if let Some(existing) = generator.definitions().get(&name) {
            assert_eq!(existing, def.as_value()); // typename not unique
        } else {
            generator
                .definitions_mut()
                .insert(name.to_string(), def.into());
        }
        let mut reference = schemars::Schema::new_ref(format!("#/$defs/{name}"));
        push_tree_leaf(&mut reference, node.data.0.internal().is_none());
        if node.data.0.node_meta_value("nullable") == Some("true") {
            reference = nullable_schema(reference);
        }
        if maybe_absent {
            reference.insert("tree-maybe-absent".to_string(), true.into());
        }
        return Some(reference);
    }
    if node.data.0.node_meta_value("nullable") == Some("true") {
        sch = nullable_schema(sch);
    }
    if maybe_absent {
        sch.insert("tree-maybe-absent".to_string(), true.into());
    }
    Some(sch)
}

impl ReflectJsonSchema for TraceNode {
    fn json_schema(&self, generator: &mut SchemaGenerator) -> Option<schemars::Schema> {
        node_json_schema(self, None, generator)
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
        assert_eq!(sch.insert("tree-leaf".to_string(), true.into()), None);
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
            .map(|(name, value)| (name.clone(), value.json_schema(&mut generator).into()))
            .collect();
        generator.definitions_mut().extend(defs);

        types.normalize()?;
        let mut root = node_json_schema(types.root(), sample.as_ref(), &mut generator).ok_or(
            serde_reflection::Error::UnknownFormatInContainer("reflection incomplete".to_string()),
        )?;
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
