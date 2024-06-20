use miniconf::{Packed, Traversal, Tree, TreeKey, TreeSerialize};

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
    assert_eq!(Settings::path(Packed::EMPTY, &mut path, "/"), Ok(0));
    path.clear();

    // Check path-packed round trip.
    for iter_path in Settings::iter_paths::<String>("/")
        .count()
        .map(Result::unwrap)
    {
        let (packed, _depth) = Settings::packed(iter_path.split("/").skip(1)).unwrap();
        Settings::path(packed, &mut path, "/").unwrap();
        assert_eq!(path, iter_path);
        println!(
            "{path} {iter_path}, {:#06b} {} {_depth}",
            packed.get() >> 60,
            packed.into_lsb().get()
        );
        path.clear();
    }
    println!(
        "{:?}",
        Settings::iter_packed()
            .map(|p| p.unwrap().into_lsb().get())
            .collect::<Vec<_>>()
    );

    // Check that Packed `marker + 0b0` is equivalent to `/a`
    assert_eq!(
        Settings::path(Packed::from_lsb(0b10.try_into().unwrap()), &mut path, "/"),
        Ok(1)
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
        S::iter_paths::<String>("/")
            .map(Result::unwrap)
            .collect::<Vec<_>>(),
        ["/foo"]
    );
    assert_eq!(S::iter_indices().collect::<Vec<_>>(), [([1, 0], 1)]);
    let (p, depth) = S::packed([1]).unwrap();
    assert_eq!((p.into_lsb().get(), depth), (0b11, 1));
    assert_eq!(
        S::iter_packed()
            .map(|p| p.unwrap().into_lsb().get())
            .collect::<Vec<_>>(),
        [0b11]
    );
}

#[test]
fn zero_key() {
    assert_eq!(
        Option::<()>::iter_packed()
            .next()
            .unwrap()
            .unwrap()
            .into_lsb()
            .get(),
        0b1
    );

    assert_eq!(
        <[usize; 1]>::iter_packed()
            .next()
            .unwrap()
            .unwrap()
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
