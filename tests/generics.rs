#![cfg(feature = "json-core")]

use miniconf::{JsonCoreSlash, Miniconf};
use serde::{Deserialize, Serialize};

#[test]
fn generic_type() {
    #[derive(Miniconf, Default)]
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
    #[derive(Miniconf, Default)]
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
    #[derive(Miniconf, Default)]
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
    #[derive(Miniconf, Default)]
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
fn test_failure() {
    #[derive(Miniconf)]
    struct S<T>(#[miniconf(defer)] [T; 0]);
    #[derive(Miniconf)]
    struct R<T>(
        // this applies the wrong bound U: Miniconf<1>, it should be U: SerDe
        #[miniconf(defer(2))] S<T>,
        // adding the missing bound indirectly is a workaround
        // commenting this out breaks it
        #[miniconf(defer)] [T; 0],
    );
}
