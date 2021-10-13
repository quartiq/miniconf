use miniconf::{Miniconf, MiniconfAtomic};
use serde::Deserialize;

#[test]
fn generic_type() {
    #[derive(Miniconf, Default)]
    struct Settings<T: Miniconf> {
        pub data: T,
    }

    let mut settings = Settings::<f32>::default();
    settings
        .string_set("data".split('/').peekable(), b"3.0")
        .unwrap();
    assert_eq!(settings.data, 3.0);
}

#[test]
fn generic_array() {
    #[derive(Miniconf, Default)]
    struct Settings<T: Miniconf> {
        pub data: [T; 2],
    }

    let mut settings = Settings::<f32>::default();
    settings
        .string_set("data/0".split('/').peekable(), b"3.0")
        .unwrap();

    assert_eq!(settings.data[0], 3.0);
}

#[test]
fn generic_struct() {
    #[derive(Miniconf, Default)]
    struct Settings<T: Miniconf> {
        pub inner: T,
    }

    #[derive(MiniconfAtomic, Deserialize, Default)]
    struct Inner {
        pub data: f32,
    }

    let mut settings = Settings::<Inner>::default();
    settings
        .string_set("inner".split('/').peekable(), b"{\"data\": 3.0}")
        .unwrap();

    assert_eq!(settings.inner.data, 3.0);
}
