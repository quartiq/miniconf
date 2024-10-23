use miniconf::{json, Deserialize, Leaf, Metadata, Serialize, Tree, TreeKey};

#[test]
fn generic_type() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub data: Leaf<T>,
    }

    let mut settings = Settings::<f32>::default();
    json::set(&mut settings, "/data", b"3.0").unwrap();
    assert_eq!(*settings.data, 3.0);

    // Test metadata
    let metadata = Settings::<f32>::traverse_all::<Metadata>().unwrap();
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "data".len());
    assert_eq!(metadata.count, 1);
}

#[test]
fn generic_array() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub data: [Leaf<T>; 2],
    }

    let mut settings = Settings::<f32>::default();
    json::set(&mut settings, "/data/0", b"3.0").unwrap();

    assert_eq!(*settings.data[0], 3.0);

    // Test metadata
    let metadata = Settings::<f32>::traverse_all::<Metadata>().unwrap();
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length("/"), "/data/0".len());
    assert_eq!(metadata.count, 2);
}

#[test]
fn generic_struct() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub inner: Leaf<T>,
    }

    #[derive(Serialize, Deserialize, Default)]
    struct Inner {
        pub data: f32,
    }

    let mut settings = Settings::<Inner>::default();
    json::set(&mut settings, "/inner", b"{\"data\": 3.0}").unwrap();

    assert_eq!(settings.inner.data, 3.0);

    // Test metadata
    let metadata = Settings::<Inner>::traverse_all::<Metadata>().unwrap();
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length("/"), "/inner".len());
    assert_eq!(metadata.count, 1);
}

#[test]
fn generic_atomic() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        atomic: Leaf<Inner<T>>,
        opt: [[Leaf<Option<T>>; 0]; 0],
        opt1: [[Option<Leaf<T>>; 0]; 0],
    }

    #[derive(Deserialize, Serialize, Default)]
    struct Inner<T> {
        inner: [T; 5],
    }

    let mut settings = Settings::<f32>::default();
    json::set(&mut settings, "/atomic", b"{\"inner\": [3.0, 0, 0, 0, 0]}").unwrap();

    assert_eq!(settings.atomic.inner[0], 3.0);

    // Test metadata
    let metadata = Settings::<f32>::traverse_all::<Metadata>().unwrap();
    assert_eq!(metadata.max_depth, 3);
    assert_eq!(metadata.max_length("/"), "/opt1/0/0".len());
}

#[test]
fn test_depth() {
    #[derive(Tree)]
    struct S<T>(Option<Option<T>>);
    // works as array implements Tree
    S::<[Leaf<u32>; 1]>::traverse_all::<Metadata>().unwrap();
}
