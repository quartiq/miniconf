#![cfg(feature = "json")]

use miniconf::{Miniconf, SerDe};
use serde::{Deserialize, Serialize};

#[test]
fn generic_type() {
    #[derive(Miniconf, Default)]
    struct Settings<T> {
        pub data: T,
    }

    let mut settings = Settings::<f32>::default();
    settings.set("/data", b"3.0").unwrap();
    assert_eq!(settings.data, 3.0);

    // Test metadata
    let metadata = Settings::<f32>::metadata(1);
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "/data".len());
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
    settings.set("/data/0", b"3.0").unwrap();

    assert_eq!(settings.data[0], 3.0);

    // Test metadata
    let metadata = Settings::<f32>::metadata(1);
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
    settings.set("/inner", b"{\"data\": 3.0}").unwrap();

    assert_eq!(settings.inner.data, 3.0);

    // Test metadata
    let metadata = Settings::<Inner>::metadata(1);
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "/inner".len());
    assert_eq!(metadata.count, 1);
}

#[test]
fn generic_atomic() {
    #[derive(Miniconf, Default)]
    struct Settings<T> {
        pub atomic: Inner<T>,
    }

    #[derive(Deserialize, Serialize, Default)]
    struct Inner<T> {
        pub inner: [T; 5],
    }

    let mut settings = Settings::<f32>::default();
    settings
        .set("/atomic", b"{\"inner\": [3.0, 0, 0, 0, 0]}")
        .unwrap();

    assert_eq!(settings.atomic.inner[0], 3.0);

    // Test metadata
    let metadata = Settings::<f32>::metadata(1);
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "/atomic".len());
}
