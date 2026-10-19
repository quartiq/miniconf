#![cfg(feature = "schema")]

use miniconf::{
    Tree,
    json::to_json_value,
    json_schema::{AllowAbsent, TreeJsonSchema},
};
use schemars::transform::Transform;

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
