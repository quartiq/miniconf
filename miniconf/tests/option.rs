use miniconf::TreeSchema;
use miniconf::{KeyError, Leaf, SerdeError, Tree, ValueError, json_core, leaf};
#[cfg(all(feature = "schema", feature = "sem"))]
use schemars::transform::Transform;

mod common;
use common::*;

#[derive(PartialEq, Debug, Clone, Default, Tree)]
struct Inner {
    data: u32,
}

#[derive(Debug, Clone, Default, Tree)]
struct Settings {
    value: Option<Inner>,
}

#[test]
fn just_option() {
    assert_eq!(paths::<Option::<u32>, 1>(), [""]);
}

#[test]
fn option_get_set_none() {
    let mut settings = Settings::default();
    let mut data = [0; 100];

    // Check that if the option is None, the value cannot be get or set.
    settings.value.take();
    assert_eq!(
        json_core::get(&settings, "/value_foo", &mut data),
        Err(KeyError::NotFound.into())
    );
    assert_eq!(
        json_core::get(&settings, "/value", &mut data),
        Err(ValueError::Absent.into())
    );
    // The Absent field indicates at which depth the variant was absent
    assert_eq!(
        json_core::set(&mut settings, "/value/data", b"5"),
        Err(ValueError::Absent.into())
    );
}

#[test]
fn option_get_set_some() {
    let mut settings = Settings::default();

    // Check that if the option is Some, the value can be get or set.
    settings.value.replace(Inner { data: 5 });

    set_get(&mut settings, "/value/data", b"7");
    assert_eq!(settings.value.unwrap().data, 7);
}

#[test]
fn option_iterate_some_none() {
    assert_eq!(paths::<Settings, 3>(), ["/value/data"]);
}

#[test]
fn option_test_normal_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        data: Leaf<Option<u32>>,
    }
    assert_eq!(paths::<S, 1>(), ["/data"]);

    let mut s = S::default();
    assert!(s.data.is_none());

    set_get(&mut s, "/data", b"7");
    assert_eq!(*s.data, Some(7));

    set_get(&mut s, "/data", b"null");
    assert!(s.data.is_none());
}

#[test]
fn option_test_defer_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        data: Option<u32>,
    }
    assert_eq!(paths::<S, 1>(), ["/data"]);

    let mut s = S::default();
    assert!(s.data.is_none());

    assert!(json_core::set(&mut s, "/data", b"7").is_err());
    s.data = Some(0);
    set_get(&mut s, "/data", b"7");
    assert_eq!(s.data, Some(7));

    assert!(json_core::set(&mut s, "/data", b"null").is_err());
}

#[test]
fn option_test_nullable_option() {
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        #[tree(with = leaf, meta(nullable))]
        data: Option<u32>,
    }
    assert_eq!(paths::<S, 1>(), ["/data"]);

    let mut s = S::default();
    set_get(&mut s, "/data", b"7");
    assert_eq!(s.data, Some(7));

    set_get(&mut s, "/data", b"null");
    assert_eq!(s.data, None);
}

#[test]
fn option_test_nullable_root() {
    #[derive(Copy, Clone, Default, Tree)]
    #[tree(flatten)]
    struct S(#[tree(with = leaf, meta(nullable))] Option<u32>);

    let mut s = S::default();
    set_get(&mut s, "", b"7");
    assert_eq!(s.0, Some(7));

    set_get(&mut s, "", b"null");
    assert_eq!(s.0, None);
}

#[test]
fn option_absent() {
    #[derive(Copy, Clone, Default, Tree)]
    struct I(());

    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        d: Option<u32>,
        dm: Option<I>,
    }

    let mut s = S::default();
    assert_eq!(
        json_core::set(&mut s, "/d", b"7"),
        Err(ValueError::Absent.into())
    );
    // Check precedence
    assert_eq!(
        json_core::set(&mut s, "/d", b""),
        Err(ValueError::Absent.into())
    );
    assert_eq!(
        json_core::set(&mut s, "/d/foo", b"7"),
        Err(ValueError::Absent.into())
    );
    assert_eq!(
        json_core::set(&mut s, "", b"7"),
        Err(KeyError::TooShort.into())
    );
    s.d = Some(3);
    assert_eq!(json_core::set(&mut s, "/d", b"7"), Ok(1));
    assert_eq!(
        json_core::set(&mut s, "/d/foo", b"7"),
        Err(KeyError::TooLong.into())
    );
    assert!(matches!(
        json_core::set(&mut s, "/d", b""),
        Err(SerdeError::Inner(_))
    ));
    assert_eq!(json_core::set(&mut s, "/d", b"7 "), Ok(2));
    assert_eq!(json_core::set(&mut s, "/d", b" 7"), Ok(2));
    assert!(matches!(
        json_core::set(&mut s, "/d", b"7i"),
        Err(SerdeError::Finalization(_))
    ));
}

#[test]
fn array_option() {
    // This tests that no invalid bounds are inferred for Options and Options in arrays.
    #[allow(dead_code)]
    #[derive(Copy, Clone, Default, Tree)]
    struct S {
        a: Option<u32>,
        b: [Leaf<Option<u32>>; 1],
        c: [Option<u32>; 1],
        d: [Option<Leaf<u32>>; 1],
    }
}

#[test]
fn option_nullable_meta() {
    #[derive(Copy, Clone, Default, Tree)]
    struct NullableField {
        #[tree(with = leaf, meta(nullable))]
        data: Option<u32>,
    }

    let miniconf::Internal::Named(children) = NullableField::SCHEMA.internal().unwrap() else {
        panic!("expected named internal schema");
    };
    assert_eq!(children[0].name(), "data");
    assert_eq!(children[0].edge_meta().get("nullable"), Some("true"));

    #[derive(Copy, Clone, Default, Tree)]
    #[tree(flatten)]
    struct NullableRoot(#[tree(with = leaf, meta(nullable))] Option<u32>);

    assert_eq!(
        NullableRoot::SCHEMA.node_meta().get("nullable"),
        Some("true")
    );
}

#[cfg(feature = "sem")]
#[test]
fn option_sem() {
    let schema = Option::<u32>::SCHEMA;
    assert_eq!(schema.sem().unwrap().ty(), Some(miniconf::Ty::U32));
    assert!(schema.sem().unwrap().maybe_absent());

    let schema = Option::<Inner>::SCHEMA;
    assert_eq!(schema.sem().unwrap().ty(), None);
    assert!(schema.sem().unwrap().maybe_absent());
    let miniconf::Internal::Named(children) = schema.internal().unwrap() else {
        panic!("expected named internal schema");
    };
    assert_eq!(children[0].name(), "data");
}

#[cfg(all(feature = "schema", feature = "sem"))]
#[test]
fn option_json_schema_matches_omitted_named_child() {
    use miniconf::{
        json::to_json_value,
        json_schema::{AllowAbsent, TreeJsonSchema},
    };

    let settings = Settings::default();
    let json = to_json_value(&settings).unwrap();
    assert_eq!(json, serde_json::json!({}));

    let mut schema = TreeJsonSchema::new(Some(&settings)).unwrap();
    assert_eq!(schema.root.get("tree-leaf"), None);
    assert_eq!(
        schema
            .root
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .and_then(|properties| properties.get("value"))
            .and_then(|value| value.get("tree-leaf")),
        None
    );
    AllowAbsent.transform(&mut schema.root);
    jsonschema::validator_for(schema.root.as_value())
        .unwrap()
        .validate(&json)
        .unwrap();
}

#[cfg(all(feature = "schema", feature = "sem"))]
#[test]
fn option_json_schema_matches_nullable_leaf() {
    use miniconf::{
        json::to_json_value,
        json_schema::{AllowAbsent, TreeJsonSchema},
    };

    #[derive(Copy, Clone, Default, Tree)]
    struct Settings {
        #[tree(with = leaf, meta(nullable))]
        value: Option<u32>,
    }

    let settings = Settings::default();
    let json = to_json_value(&settings).unwrap();
    assert_eq!(json, serde_json::json!({"value": null}));

    let mut schema = TreeJsonSchema::new(Some(&settings)).unwrap();
    assert_eq!(schema.root.get("tree-leaf"), None);
    assert_eq!(
        schema
            .root
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .and_then(|properties| properties.get("value"))
            .and_then(|value| value.get("tree-leaf")),
        Some(&serde_json::json!(true))
    );
    AllowAbsent.transform(&mut schema.root);
    jsonschema::validator_for(schema.root.as_value())
        .unwrap()
        .validate(&json)
        .unwrap();
}
