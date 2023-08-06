#![cfg(feature = "json-core")]

use miniconf::{JsonCoreSlash, Tree, TreeKey};
use serde::{Deserialize, Serialize};

#[test]
fn generic_type() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub data: T,
    }

    let mut settings = Settings::<f32>::default();
    settings.set_json("/data", b"3.0").unwrap();
    assert_eq!(settings.data, 3.0);

    // Test metadata
    let metadata = Settings::<f32>::metadata();
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "data".len());
    assert_eq!(metadata.count, 1);
}

#[test]
fn generic_array() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        #[miniconf(defer)]
        pub data: [T; 2],
    }

    let mut settings = Settings::<f32>::default();
    settings.set_json("/data/0", b"3.0").unwrap();

    assert_eq!(settings.data[0], 3.0);

    // Test metadata
    let metadata = Settings::<f32>::metadata().separator("/");
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length, "/data/0".len());
    assert_eq!(metadata.count, 2);
}

#[test]
fn generic_struct() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub inner: T,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct Inner {
        pub data: f32,
    }

    let mut settings = Settings::<Inner>::default();
    settings.set_json("/inner", b"{\"data\": 3.0}").unwrap();

    assert_eq!(settings.inner.data, 3.0);

    // Test metadata
    let metadata = Settings::<Inner>::metadata().separator("/");
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "/inner".len());
    assert_eq!(metadata.count, 1);
}

#[test]
fn generic_atomic() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        atomic: Inner<T>,
        #[miniconf(defer(2))]
        opt: [[Option<T>; 0]; 0],
        #[miniconf(defer(3))]
        opt1: [[Option<T>; 0]; 0],
    }

    #[derive(Deserialize, Serialize, Default)]
    struct Inner<T> {
        inner: [T; 5],
    }

    let mut settings = Settings::<f32>::default();
    settings
        .set_json("/atomic", b"{\"inner\": [3.0, 0, 0, 0, 0]}")
        .unwrap();

    assert_eq!(settings.atomic.inner[0], 3.0);

    // Test metadata
    let metadata = Settings::<f32>::metadata().separator("/");
    assert_eq!(metadata.max_depth, 3);
    assert_eq!(metadata.max_length, "/opt1/0/0".len());
}

#[test]
fn test_derive_macro_bound_failure() {
    // The derive macro uses a simplistic approach to adding bounds
    // for generic types.
    // This is analogous to other standard derive macros.
    // See also the documentation for the [Tree] trait
    // and the code comments in the derive macro ([StructField::walk_type]
    // on adding bounds to generics.
    // This test below shows the issue and tests whether the workaround of
    // adding the required traits by hand works.
    type A<T> = [[T; 0]; 0];
    #[derive(Tree)]
    struct S<T: serde::Serialize + serde::de::DeserializeOwned>(
        // this wrongly infers T: Tree<1> instead of T: SerDe
        // adding the missing bound is a workaround
        #[miniconf(defer(2))] A<T>,
    );
}

#[test]
fn test_depth() {
    #[derive(Tree)]
    struct S<T>(#[miniconf(defer(3))] Option<Option<T>>);
    // works as array implements Tree<1>
    S::<[u32; 1]>::metadata();
    // does not compile as u32 does not implement Tree<1>
    // S::<u32>::metadata();
}
