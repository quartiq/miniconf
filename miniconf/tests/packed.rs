use miniconf::{
    Indices, Leaf, Metadata, Node, Packed, Path, Traversal, Tree, TreeKey, TreeSerialize,
};

#[derive(Tree, Default)]
struct Settings {
    a: Leaf<f32>,
    b: [Leaf<f32>; 2],
}

#[test]
fn packed() {
    // Check empty being too short
    assert_eq!(
        Settings::transcode::<Path<String, '/'>, _>(Packed::EMPTY),
        Ok((Path::default(), Node::internal(0)))
    );

    // Check path-packed round trip.
    for (iter_path, _node) in Settings::nodes::<Path<String, '/'>, 2>()
        .exact_size()
        .map(Result::unwrap)
    {
        let (packed, node) = Settings::transcode::<Packed, _>(&iter_path).unwrap();
        let (path, _node) = Settings::transcode::<Path<String, '/'>, _>(packed).unwrap();
        assert_eq!(path, iter_path);
        println!(
            "{path:?} {iter_path:?}, {:#06b} {} {node:?}",
            packed.get() >> 60,
            packed.into_lsb().get()
        );
    }
    println!(
        "{:?}",
        Settings::nodes::<Packed, 2>()
            .map(|p| p.unwrap().0.into_lsb().get())
            .collect::<Vec<_>>()
    );

    // Check that Packed `marker + 0b0` is equivalent to `/a`
    let a = Packed::from_lsb(0b10.try_into().unwrap());
    let (path, node) = Settings::transcode::<Path<String, '/'>, _>(a).unwrap();
    assert_eq!(node, Node::leaf(1));
    assert_eq!(path.as_str(), "/a");
}

#[test]
fn top() {
    #[derive(Tree)]
    struct S {
        baz: [Leaf<i32>; 1],
        foo: Leaf<i32>,
    }
    assert_eq!(
        S::nodes::<Path<String, '/'>, 2>()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        ["/baz/0", "/foo"]
    );
    assert_eq!(
        S::nodes::<Indices<_>, 2>()
            .map(|p| p.unwrap())
            .collect::<Vec<_>>(),
        [
            (Indices([0, 0]), Node::leaf(2)),
            (Indices([1, 0]), Node::leaf(1))
        ]
    );
    let (p, node) = S::transcode::<Packed, _>([1usize]).unwrap();
    assert_eq!((p.into_lsb().get(), node), (0b11, Node::leaf(1)));
    assert_eq!(
        S::nodes::<Packed, 2>()
            .map(|p| p.unwrap().0.into_lsb().get())
            .collect::<Vec<_>>(),
        [0b100, 0b11]
    );
}

#[test]
fn zero_key() {
    assert_eq!(
        Option::<Leaf<()>>::nodes::<Packed, 2>()
            .next()
            .unwrap()
            .unwrap()
            .0
            .into_lsb()
            .get(),
        0b1
    );

    assert_eq!(
        <[Leaf<usize>; 1]>::nodes::<Packed, 2>()
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
    let mut a11 = [[Leaf(0)]];
    let mut a22 = [[Leaf(0); 2]; 2];
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
            TreeSerialize::serialize_by_key(
                &mut a11,
                Packed::from_lsb((0b1 << depth).try_into().unwrap()),
                &mut ser
            ),
            *result
        );
        assert_eq!(
            TreeSerialize::serialize_by_key(
                &mut a22,
                Packed::from_lsb((0b1 << depth).try_into().unwrap()),
                &mut ser
            ),
            *result
        );
    }
}

#[test]
fn size() {
    // Play with the worst cases for 32 bit Packed
    // Bit-hungriest type would be [T; 0] or () but (a) those are forbidden (internal without leaves) and (b) they doesn't have any keys
    // so won't recurse in Transcode or consume from Keys
    // Then [T; 1] which takes one bit per level (not 0 bits, to distinguish empty Packed)
    // Worst case for a 32 bit usize we need 31 array levels (marker bit).
    type A31 = [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[Leaf<()>; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1];
        1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1];
    assert_eq!(core::mem::size_of::<A31>(), 0);
    let packed = Packed::new_from_lsb(1 << 31).unwrap();
    let (path, node) = A31::transcode::<Path<String, '/'>, _>(packed).unwrap();
    assert_eq!(node, Node::leaf(31));
    assert_eq!(path.as_str().len(), 2 * 31);
    let meta: Metadata = A31::traverse_all().unwrap();
    assert_eq!(meta.max_bits, 31);
    assert_eq!(meta.max_depth, 31);
    assert_eq!(meta.count, 1usize.pow(31));
    assert_eq!(meta.max_length, 31);

    // Another way to get to 32 bit is to take 15 length-3 (2 bit) levels and one length-1 (1 bit) level to fill it, needing (3**15 ~ 14 M) storage.
    // With the unit as type, we need 0 storage but can't do much.
    type A16 =
        [[[[[[[[[[[[[[[[Leaf<()>; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 1];
    assert_eq!(core::mem::size_of::<A16>(), 0);
    let packed = Packed::new_from_lsb(1 << 31).unwrap();
    let (path, node) = A16::transcode::<Path<String, '/'>, _>(packed).unwrap();
    assert_eq!(node, Node::leaf(16));
    assert_eq!(path.as_str().len(), 2 * 16);
    let meta: Metadata = A16::traverse_all().unwrap();
    assert_eq!(meta.max_bits, 31);
    assert_eq!(meta.max_depth, 16);
    assert_eq!(meta.count, 3usize.pow(15));
    assert_eq!(meta.max_length, 16);
}
