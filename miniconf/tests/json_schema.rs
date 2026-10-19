#![cfg(feature = "schema")]

use miniconf::{
    Tree,
    json::to_json_value,
    json_schema::{AllowAbsent, TreeJsonSchema},
};
use schemars::transform::Transform;
use serde_json::json;

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
