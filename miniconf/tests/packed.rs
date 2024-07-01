use miniconf::{Indices, Node, Packed, Path, Transcode, Traversal, Tree, TreeKey, TreeSerialize};

#[derive(Tree, Default)]
struct Settings {
    a: f32,
    #[tree(depth = 1)]
    b: [f32; 2],
}

#[test]
fn packed() {
    let mut path = String::new();

    // Check empty being too short
    assert_eq!(
        Settings::transcode::<Path<String, '/'>, _>(Packed::EMPTY),
        Ok((Path::default(), Node::internal(0)))
    );
    path.clear();

    // Check path-packed round trip.
    for (iter_path, _node) in Settings::nodes::<Path<String, '/'>>()
        .exact_size()
        .map(Result::unwrap)
    {
        let (packed, node) = Settings::transcode::<Packed, _>(&iter_path).unwrap();
        Path::<_, '/'>(&mut path)
            .transcode::<Settings, 2, _>(packed)
            .unwrap();
        assert_eq!(path, iter_path.as_str());
        println!(
            "{path} {iter_path:?}, {:#06b} {} {node:?}",
            packed.get() >> 60,
            packed.into_lsb().get()
        );
        path.clear();
    }
    println!(
        "{:?}",
        Settings::nodes::<Packed>()
            .map(|p| p.unwrap().0.into_lsb().get())
            .collect::<Vec<_>>()
    );

    // Check that Packed `marker + 0b0` is equivalent to `/a`
    assert_eq!(
        Path::<_, '/'>(&mut path)
            .transcode::<Settings, 2, _>(Packed::from_lsb(0b10.try_into().unwrap())),
        Ok(Node::leaf(1))
    );
    assert_eq!(path, "/a");
    path.clear();
}

#[test]
fn top() {
    #[derive(Tree)]
    struct S {
        #[tree(depth = 1)]
        baz: [i32; 0],
        foo: i32,
    }
    assert_eq!(
        S::nodes::<Path<String, '/'>>()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        ["/foo"]
    );
    assert_eq!(
        S::nodes::<Indices<_>>()
            .map(|p| p.unwrap())
            .collect::<Vec<_>>(),
        [(Indices([1, 0]), Node::leaf(1))]
    );
    let (p, node) = S::transcode::<Packed, _>([1]).unwrap();
    assert_eq!((p.into_lsb().get(), node), (0b11, Node::leaf(1)));
    assert_eq!(
        S::nodes::<Packed>()
            .map(|p| p.unwrap().0.into_lsb().get())
            .collect::<Vec<_>>(),
        [0b11]
    );
}

#[test]
fn zero_key() {
    assert_eq!(
        Option::<()>::nodes::<Packed>()
            .next()
            .unwrap()
            .unwrap()
            .0
            .into_lsb()
            .get(),
        0b1
    );

    assert_eq!(
        <[usize; 1]>::nodes::<Packed>()
            .next()
            .unwrap()
            .unwrap()
            .0
            .into_lsb()
            .get(),
        0b10
    );

    // Check the corner case of a len=1 index where (len - 1) = 0 and zero bits would be required to encode.
    // Hence the Packed values for len=1 and len=2 are the same.
    let mut a11 = [[0]];
    let mut a22 = [[0, 0], [0, 0]];
    let mut buf = [0u8; 100];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    for (depth, result) in [
        Err(Traversal::TooShort(0).into()),
        Err(Traversal::TooShort(1).into()),
        Ok(2),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            TreeSerialize::<2>::serialize_by_key(
                &mut a11,
                Packed::from_lsb((0b1 << depth).try_into().unwrap()),
                &mut ser
            ),
            *result
        );
        assert_eq!(
            TreeSerialize::<2>::serialize_by_key(
                &mut a22,
                Packed::from_lsb((0b1 << depth).try_into().unwrap()),
                &mut ser
            ),
            *result
        );
    }
}
