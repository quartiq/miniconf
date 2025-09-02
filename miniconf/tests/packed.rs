use miniconf::{
    Indices, KeyError, Leaf, Packed, Path, Shape, Short, Track, Tree, TreeSchema, TreeSerialize,
};
mod common;

#[derive(Tree, Default)]
struct Settings {
    a: Leaf<f32>,
    b: [Leaf<f32>; 2],
}

#[test]
fn packed() {
    // Check empty being too short
    assert_eq!(
        Settings::SCHEMA
            .transcode::<Short<Path<String, '/'>>>(Packed::EMPTY)
            .unwrap(),
        Short::default()
    );

    // Check path-packed round trip.
    for iter_path in Settings::SCHEMA
        .nodes::<Path<String, '/'>, 2>()
        .exact_size()
        .map(Result::unwrap)
    {
        let packed = Settings::SCHEMA
            .transcode::<Track<Packed>>(&iter_path)
            .unwrap();
        let path = Settings::SCHEMA
            .transcode::<Path<String, '/'>>(packed.inner)
            .unwrap();
        assert_eq!(path, iter_path);
        println!(
            "{path:?} {iter_path:?}, {:#06b} {} {:?}",
            packed.inner.get() >> 60,
            packed.inner.into_lsb().get(),
            packed.depth
        );
    }
    println!(
        "{:?}",
        Settings::SCHEMA
            .nodes::<Packed, 2>()
            .map(|p| p.unwrap().into_lsb().get())
            .collect::<Vec<_>>()
    );

    // Check that Packed `marker + 0b0` is equivalent to `/a`
    let a = Packed::from_lsb(0b10.try_into().unwrap());
    let path = Settings::SCHEMA
        .transcode::<Track<Path<String, '/'>>>(a)
        .unwrap();
    assert_eq!(path.depth, 1);
    assert_eq!(path.inner.as_str(), "/a");
}

#[test]
fn top() {
    #[derive(Tree)]
    struct S {
        baz: [Leaf<i32>; 1],
        foo: Leaf<i32>,
    }
    assert_eq!(
        S::SCHEMA
            .nodes::<Path<String, '/'>, 2>()
            .map(|p| p.unwrap().into_inner())
            .collect::<Vec<_>>(),
        ["/baz/0", "/foo"]
    );
    assert_eq!(
        S::SCHEMA
            .nodes::<Indices<_>, 2>()
            .map(|p| p.unwrap())
            .collect::<Vec<_>>(),
        [
            Indices {
                data: [0, 0],
                len: 2
            },
            Indices {
                data: [1, 0],
                len: 1
            },
        ]
    );
    let p = S::SCHEMA.transcode::<Track<Packed>>([1usize]).unwrap();
    assert_eq!((p.inner.into_lsb().get(), p.depth), (0b11, 1));
    assert_eq!(
        S::SCHEMA
            .nodes::<Packed, 2>()
            .map(|p| p.unwrap().into_lsb().get())
            .collect::<Vec<_>>(),
        [0b100, 0b11]
    );
}

#[test]
fn zero_key() {
    assert_eq!(
        Option::<Leaf<()>>::SCHEMA
            .nodes::<Packed, 2>()
            .next()
            .unwrap()
            .unwrap()
            .into_lsb()
            .get(),
        0b1
    );

    assert_eq!(
        <[Leaf<usize>; 1]>::SCHEMA
            .nodes::<Packed, 2>()
            .next()
            .unwrap()
            .unwrap()
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
        Err(KeyError::TooShort.into()),
        Err(KeyError::TooShort.into()),
        Ok(()),
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
    // The max width claims below only apply to 32 bit architectures

    // Play with the worst cases for 32 bit Packed
    // Bit-hungriest type would be [T; 0] or () but (a) those are forbidden (internal without leaves) and (b) they don't have any keys
    // so won't recurse in Transcode or consume from Keys
    // Then [T; 1] which takes one bit per level (not 0 bits, to distinguish empty Packed)
    // Worst case for a 32 bit usize we need 31 array levels (marker bit).
    type A31 = [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[Leaf<()>; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1];
        1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1]; 1];
    assert_eq!(core::mem::size_of::<A31>(), 0);
    let packed = Packed::new_from_lsb(1 << 31).unwrap();
    let path = A31::SCHEMA
        .transcode::<Track<Path<String, '/'>>>(packed)
        .unwrap();
    assert_eq!(path.depth, 31);
    assert_eq!(path.inner.as_str().len(), 2 * 31);
    let meta: Shape = A31::SCHEMA.shape();
    assert_eq!(meta.max_bits, 31);
    assert_eq!(meta.max_depth, 31);
    assert_eq!(meta.count.get(), 1usize.pow(31));
    assert_eq!(meta.max_length, 31);

    // Another way to get to 32 bit is to take 15 length-3 (2 bit) levels and one length-1 (1 bit) level to fill it, needing (3**15 ~ 14 M) storage.
    // With the unit as type, we need 0 storage but can't do much.
    type A16 =
        [[[[[[[[[[[[[[[[Leaf<()>; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 3]; 1];
    assert_eq!(core::mem::size_of::<A16>(), 0);
    let packed = Packed::new_from_lsb(1 << 31).unwrap();
    let path = A16::SCHEMA
        .transcode::<Track<Path<String, '/'>>>(packed)
        .unwrap();
    assert_eq!(path.depth, 16);
    assert_eq!(path.inner.as_str().len(), 2 * 16);
    let meta: Shape = A16::SCHEMA.shape();
    assert_eq!(meta.max_bits, 31);
    assert_eq!(meta.max_depth, 16);
    assert_eq!(meta.count.get(), 3usize.pow(15));
    assert_eq!(meta.max_length, 16);
}
