use miniconf::{json, Deserialize, Leaf, Serialize, Shape, Tree, TreeSchema};

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
    let metadata: Shape = Settings::<f32>::SHAPE;
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length, "data".len());
    assert_eq!(metadata.count.get(), 1);
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
    let metadata: Shape = Settings::<f32>::SHAPE;
    assert_eq!(metadata.max_depth, 2);
    assert_eq!(metadata.max_length("/"), "/data/0".len());
    assert_eq!(metadata.count.get(), 2);
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
    let metadata: Shape = Settings::<Inner>::SHAPE;
    assert_eq!(metadata.max_depth, 1);
    assert_eq!(metadata.max_length("/"), "/inner".len());
    assert_eq!(metadata.count.get(), 1);
}

#[test]
fn generic_atomic() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        atomic: Leaf<Inner<T>>,
        opt: [[Leaf<Option<T>>; 1]; 1],
        opt1: [[Option<Leaf<T>>; 1]; 1],
    }

    #[derive(Deserialize, Serialize, Default)]
    struct Inner<T> {
        inner: [T; 5],
    }

    let mut settings = Settings::<f32>::default();
    json::set(&mut settings, "/atomic", b"{\"inner\": [3.0, 0, 0, 0, 0]}").unwrap();

    assert_eq!(settings.atomic.inner[0], 3.0);

    // Test metadata
    let metadata: Shape = Settings::<f32>::SHAPE;
    assert_eq!(metadata.max_depth, 3);
    assert_eq!(metadata.max_length("/"), "/opt1/0/0".len());
}

#[test]
fn test_depth() {
    #[derive(Tree)]
    struct S<T>(Option<Option<T>>);

    // This works as array implements TreeSchema
    let _ = S::<[Leaf<u32>; 1]>::SHAPE;

    // This does not compile as u32 does not implement TreeSchema
    //let _ = S::<u32>::SHAPE;

    // Depth is always statically known
    // .. but can't be used in const generics yet,
    //    i.e. we can't name the type.
    let _idx = [0usize; S::<[Leaf<u32>; 1]>::SHAPE.max_depth];
}
