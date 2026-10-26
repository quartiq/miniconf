#![cfg(feature = "schema")]

use miniconf::{
    Tree,
    json::to_json_value,
    json_schema::{AllowAbsent, TreeJsonSchema},
};
use schemars::transform::Transform;
use serde_json::json;

#[cfg(all(feature = "meta-node", feature = "meta-edge"))]
#[path = "../examples/common.rs"]
mod common;

#[derive(Tree, miniconf::Serialize, miniconf::Deserialize, Default)]
#[tree(meta(typename))]
struct Pair {
    #[tree(meta(unit = "V"))]
    gain: u16,
    enabled: bool,
}

#[derive(Tree, Default)]
struct Mixed {
    internal: Pair,
    atomic: miniconf::Leaf<Pair>,
    numbered: (u8, bool),
    array: [Option<u16>; 2],
    mode: Option<Result<u16, bool>>,
    #[tree(meta(nullable))]
    nullable: miniconf::Leaf<Option<Option<u16>>>,
}

#[cfg(all(feature = "meta-node", feature = "meta-edge"))]
#[test]
fn consumer_contract_fixture() {
    use miniconf::{TreeSchema, compact_schema::SchemaDefs};

    let settings = common::Settings::new();
    let mut schema = TreeJsonSchema::new(Some(&settings)).unwrap().root;
    schema.insert("default".into(), to_json_value(&settings).unwrap());
    let compact = SchemaDefs::<32>::new(common::Settings::SCHEMA).unwrap();
    let definitions: String = (0..compact.len())
        .map(|i| serde_json::to_string(&compact.definition(i).unwrap()).unwrap() + "\n")
        .collect();
    // UPDATE_FIXTURES=1 cargo test -p miniconf --features schema consumer_contract_fixture
    for (name, actual) in [
        (
            "common.json",
            serde_json::to_string_pretty(&schema).unwrap() + "\n",
        ),
        ("common.ndjson", definitions),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(name);
        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            std::fs::write(&path, &actual).unwrap();
        }
        // Named-child order determines indices; compare text, not unordered maps.
        assert_eq!(actual, std::fs::read_to_string(path).unwrap());
    }
}

#[test]
fn structured_leaves_roots_and_null_values() {
    let mut settings = Mixed::default();
    let mut schema = TreeJsonSchema::new(Some(&settings)).unwrap().root;
    AllowAbsent.transform(&mut schema);
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for value in [None, Some(None), Some(Some(7))] {
        settings.nullable.0 = value;
        validator
            .validate(&to_json_value(&settings).unwrap())
            .unwrap();
    }
    for mode in [Some(Ok(2)), Some(Err(true)), None] {
        settings.mode = mode;
        validator
            .validate(&to_json_value(&settings).unwrap())
            .unwrap();
    }
    // An atomic object has the same JSON shape as an internal named node.
    let actual = to_json_value(&settings).unwrap();
    assert_eq!(actual["internal"], actual["atomic"]);
    let root = miniconf::Leaf(Pair::default());
    let schema = TreeJsonSchema::new(Some(&root)).unwrap().root;
    jsonschema::validator_for(schema.as_value())
        .unwrap()
        .validate(&to_json_value(&root).unwrap())
        .unwrap();
}

#[test]
fn inaccessible_values_are_diagnostics_and_unresolved_formats_fail() {
    #[derive(Tree)]
    struct Hidden {
        #[tree(with = miniconf::deny)]
        value: u16,
    }
    let hidden = Hidden { value: 7 };
    assert_eq!(
        to_json_value(&hidden).unwrap(),
        json!({"value": miniconf::json::TREE_ACCESS})
    );
    // Neither value tracing nor type probing can discover a denied leaf format.
    assert!(TreeJsonSchema::new(Some(&hidden)).is_err());
}

#[test]
fn serialized_enum_leaf_variants() {
    #[derive(miniconf::Serialize, miniconf::Deserialize)]
    enum Value {
        Idle,
        Gain(u16),
        Pair(u8, bool),
        Named { enabled: bool },
    }
    let schema = TreeJsonSchema::<miniconf::Leaf<Value>>::new(None)
        .unwrap()
        .root;
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for value in [
        Value::Idle,
        Value::Gain(3),
        Value::Pair(1, true),
        Value::Named { enabled: false },
    ] {
        validator
            .validate(&to_json_value(&miniconf::Leaf(value)).unwrap())
            .unwrap();
    }
    for invalid in [json!("Unknown"), json!({"Pair": [1]}), json!({"Named": {}})] {
        assert!(!validator.is_valid(&invalid));
    }
}

#[test]
fn projections_preserve_source_tree_facts() {
    use miniconf::{Internal, Schema, TreeSchema, compact_schema::SchemaDefs};
    use serde_json::Value;

    fn compare(source: &Schema, compact: &Value, usage: &Value, defs: &[Value], json: &Value) {
        let usage = usage.get("anyOf").map_or(usage, |branches| &branches[0]);
        let node = json.pointer(&usage["$ref"].as_str().unwrap()[1..]).unwrap();
        let metadata = json!(source.node_meta());
        assert_eq!(compact.get("m").cloned().unwrap_or(json!({})), metadata);
        assert_eq!(
            node.get("tree-node-meta").cloned().unwrap_or(json!({})),
            metadata
        );
        let sem = source.sem();
        assert_eq!(
            node["tree-maybe-absent"] == true,
            sem.is_some_and(|s| s.maybe_absent())
        );
        assert_eq!(
            compact["s"]["maybe_absent"] == true,
            sem.is_some_and(|s| s.maybe_absent())
        );
        assert_eq!(node["tree-oneof"] == true, sem.is_some_and(|s| s.oneof()));
        assert_eq!(
            compact["s"]["oneof"] == true,
            sem.is_some_and(|s| s.oneof())
        );
        let (kind, short, children): (_, _, Vec<(&Value, &Value)>) = match source.internal() {
            None => {
                assert_eq!(node["tree-kind"], "leaf");
                assert!(compact.get("i").is_none());
                return;
            }
            Some(Internal::Named(fields)) => {
                let names: Vec<_> = fields.iter().map(|f| f.name()).collect();
                assert_eq!(
                    node["properties"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    names
                );
                assert_eq!(
                    compact["i"]["c"]
                        .as_object()
                        .unwrap()
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    names
                );
                (
                    "named",
                    "n",
                    names
                        .iter()
                        .map(|n| (&compact["i"]["c"][n], &node["properties"][n]))
                        .collect(),
                )
            }
            Some(Internal::Numbered(fields)) => {
                assert_eq!(node["prefixItems"].as_array().unwrap().len(), fields.len());
                assert_eq!(compact["i"]["c"].as_array().unwrap().len(), fields.len());
                (
                    "numbered",
                    "d",
                    (0..fields.len())
                        .map(|i| (&compact["i"]["c"][i], &node["prefixItems"][i]))
                        .collect(),
                )
            }
            Some(Internal::Homogeneous(field)) => {
                assert_eq!(node["minItems"], json!(field.len().get()));
                assert_eq!(node["maxItems"], json!(field.len().get()));
                assert_eq!(compact["i"]["l"], json!(field.len().get()));
                (
                    "homogeneous",
                    "h",
                    vec![(&compact["i"]["c"], &node["items"])],
                )
            }
        };
        assert_eq!(node["tree-kind"], kind);
        assert_eq!(compact["i"]["k"], short);
        let internal = source.internal().unwrap();
        for (i, (child, (compact_use, json_use))) in internal.schemata().zip(children).enumerate() {
            let metadata = json!(internal.get_edge_meta(i));
            assert_eq!(compact_use.get("m").cloned().unwrap_or(json!({})), metadata);
            assert_eq!(
                json_use.get("tree-edge-meta").cloned().unwrap_or(json!({})),
                metadata
            );
            let id = compact_use
                .get("r")
                .unwrap_or(compact_use)
                .as_u64()
                .unwrap() as usize;
            compare(child, &defs[id], json_use, defs, json);
        }
    }

    let source = Mixed::SCHEMA;
    let compact = SchemaDefs::<32>::new(source).unwrap();
    let defs: Vec<_> = (0..compact.len())
        .map(|i| json!(compact.definition(i).unwrap()))
        .collect();
    let json = TreeJsonSchema::new(Some(&Mixed::default())).unwrap().root;
    compare(
        source,
        defs.last().unwrap(),
        json.as_value(),
        &defs,
        json.as_value(),
    );
}

#[test]
fn leaf_schemas_match_json_serialization() {
    #[derive(Tree)]
    struct Settings {
        #[tree(with = miniconf::leaf)]
        sequence: Vec<u32>,
        #[tree(with = miniconf::leaf)]
        tuple: (u32, bool),
        #[tree(with = miniconf::leaf)]
        optional: Option<()>,
        #[tree(with = miniconf::leaf)]
        mapping: std::collections::BTreeMap<u32, bool>,
    }
    let mut settings = Settings {
        sequence: vec![1],
        tuple: (2, true),
        optional: None,
        mapping: [(3, true)].into_iter().collect(),
    };
    let schema = TreeJsonSchema::new(Some(&settings)).unwrap().root;
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for optional in [None, Some(())] {
        settings.optional = optional;
        validator
            .validate(&to_json_value(&settings).unwrap())
            .unwrap();
    }
    let actual = to_json_value(&settings).unwrap();
    assert_eq!(actual["mapping"], json!({"3": true}));
    for (field, invalid) in [
        ("sequence", json!(17)),
        ("sequence", json!([false])),
        ("tuple", json!([])),
        ("tuple", json!([2])),
        ("tuple", json!([2, true, 3])),
        ("mapping", json!([[3, true]])),
        ("mapping", json!({"3": 1})),
    ] {
        let mut value = actual.clone();
        value[field] = invalid;
        assert!(!validator.is_valid(&value), "accepted {value}");
    }
}

#[test]
fn variants_and_array_presence_are_independent_of_the_sample() {
    #[derive(Tree)]
    enum Mode {
        A(Option<u32>),
        B(Option<bool>),
    }
    #[derive(Tree)]
    struct Settings {
        mode: Mode,
        values: [Option<u32>; 2],
    }
    let first = Settings {
        mode: Mode::A(None),
        values: [None, Some(1)],
    };
    let second = Settings {
        mode: Mode::B(Some(true)),
        values: [Some(2), None],
    };
    let mut schema = TreeJsonSchema::new(Some(&first)).unwrap().root;
    assert_eq!(schema, TreeJsonSchema::new(Some(&second)).unwrap().root);
    AllowAbsent.transform(&mut schema);
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for value in [&first, &second] {
        validator.validate(&to_json_value(value).unwrap()).unwrap();
    }
    assert!(
        !validator.is_valid(&serde_json::json!({"mode": {"A": 1, "B": true}, "values": [1, 2]}))
    );
    assert!(!validator.is_valid(&serde_json::json!({"mode": {}, "values": [1]})));
    assert!(!validator.is_valid(&serde_json::json!({"mode": {"unknown": 1}, "values": [1, 2]})));
}
