#![cfg(feature = "json-core")]

use miniconf::{Error, Packed, Tree, TreeKey, TreeSerialize};

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
        Settings::path(Packed::EMPTY, &mut path, "/"),
        Err(Error::TooShort(0))
    );
    path.clear();

    // Check path-packed round trip.
    for iter_path in Settings::iter_paths::<String>("/").map(Result::unwrap) {
        let (packed, _depth) = Settings::packed(iter_path.split("/").skip(1)).unwrap();
        Settings::path(packed, &mut path, "/").unwrap();
        assert_eq!(path, iter_path);
        path.clear();
    }
    println!(
        "{:?}",
        Settings::iter_packed()
            .map(Result::unwrap)
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
fn zero_key() {
    // Check the corner case of a len=1 index where (len - 1) = 0 and zero bits would be required to encode.
    // Hence the Packed values for len=1 and len=2 are the same.
    let mut a11 = [[0]];
    let mut a22 = [[0, 0], [0, 0]];
    let mut buf = [0u8; 100];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    for (depth, result) in [Err(Error::TooShort(0)), Err(Error::TooShort(1)), Ok(2)]
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
