use miniconf::{
    Deserialize, ExactSize, Leaf, NodeIter, Serialize, Shape, Tree, TreeSchema, json_core,
};

#[test]
fn generic_type() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub data: T,
    }

    let mut settings = Settings::<f32>::default();
    json_core::set(&mut settings, "/data", b"3.0").unwrap();
    assert_eq!(settings.data, 3.0);

    const SHAPE: Shape = Settings::<f32>::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 1);
    assert_eq!(SHAPE.max_length, "data".len());
    assert_eq!(SHAPE.count.get(), 1);
}

#[test]
fn generic_array() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        pub data: [T; 2],
    }

    let mut settings = Settings::<f32>::default();
    json_core::set(&mut settings, "/data/0", b"3.0").unwrap();

    assert_eq!(settings.data[0], 3.0);

    const SHAPE: Shape = Settings::<f32>::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 2);
    assert_eq!(SHAPE.max_length("/"), "/data/0".len());
    assert_eq!(SHAPE.count.get(), 2);
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

    let mut settings = Settings::<Leaf<Inner>>::default();
    json_core::set(&mut settings, "/inner", b"{\"data\": 3.0}").unwrap();

    assert_eq!(settings.inner.data, 3.0);

    const SHAPE: Shape = Settings::<f32>::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 1);
    assert_eq!(SHAPE.max_length("/"), "/inner".len());
    assert_eq!(SHAPE.count.get(), 1);
}

#[test]
fn generic_atomic() {
    #[derive(Tree, Default)]
    struct Settings<T> {
        atomic: Leaf<Inner<T>>,
        opt: [[Leaf<Option<T>>; 1]; 1],
        opt1: [[Option<T>; 1]; 1],
    }

    #[derive(Deserialize, Serialize, Default)]
    struct Inner<T> {
        inner: [T; 5],
    }

    let mut settings = Settings::<f32>::default();
    json_core::set(&mut settings, "/atomic", b"{\"inner\": [3.0, 0, 0, 0, 0]}").unwrap();

    assert_eq!(settings.atomic.inner[0], 3.0);

    // Test metadata
    const SHAPE: Shape = Settings::<f32>::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 3);
    assert_eq!(SHAPE.max_length("/"), "/opt1/0/0".len());
}

#[test]
fn derived_impl_generics_do_not_collide() {
    #[derive(Tree, Default)]
    struct Settings<'de, S, D> {
        value: S,
        other: D,
        #[tree(skip)]
        _lifetime: core::marker::PhantomData<&'de ()>,
    }

    let mut settings = Settings::<u32, u16>::default();
    json_core::set(&mut settings, "/value", b"3").unwrap();
    json_core::set(&mut settings, "/other", b"5").unwrap();

    assert_eq!(settings.value, 3);
    assert_eq!(settings.other, 5);
}

#[test]
fn test_depth() {
    #[derive(Tree)]
    struct S<T>(Option<Option<T>>);

    // This works as array implements TreeSchema
    let _ = S::<[u32; 1]>::SCHEMA.shape();

    // u32 implements TreeSchema as well
    let _ = S::<u32>::SCHEMA.shape();

    // Depth is always statically known
    // .. but can't be used in const generics yet,
    //    i.e. we can't name the type.
    let _ = [0usize; S::<[u32; 1]>::SCHEMA.shape().max_depth];

    const _: ExactSize<NodeIter<(), 2>> = <S<[u32; 1]>>::SCHEMA.nodes();
}

#[cfg(all(feature = "schema", feature = "meta-node"))]
#[test]
fn schema_typename_reuse_and_collision() {
    use miniconf::json_schema::TreeJsonSchema;

    #[derive(Tree, Default)]
    #[tree(meta(typename))]
    struct Channel<T> {
        value: T,
    }
    #[derive(Tree, Default)]
    struct Settings<T> {
        a: Channel<u8>,
        b: Channel<T>,
    }

    let schema = TreeJsonSchema::<Settings<u8>>::new(Some(&Settings::default())).unwrap();
    assert!(
        schema
            .generator
            .definitions()
            .contains_key("tree-internal-Channel")
    );
    assert_eq!(
        schema.root.as_value()["properties"]["a"]["$ref"],
        schema.root.as_value()["properties"]["b"]["$ref"]
    );
    let error = TreeJsonSchema::<Settings<bool>>::new(Some(&Settings::default()))
        .err()
        .expect("different definitions must not share a name");
    assert!(matches!(error, serde_reflection::Error::Custom(message)
        if message == "Conflicting schema definitions for tree-internal-Channel"));
}
