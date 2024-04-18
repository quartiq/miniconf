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
    let mut p = String::new();

    assert_eq!(
        Settings::path(Packed::default(), &mut p, "/"),
        Err(Error::TooShort(0))
    );
    p.clear();

    for q in Settings::iter_paths::<String>("/") {
        let q = q.unwrap();
        let (a, _d) = Settings::packed(q.split("/").skip(1)).unwrap();
        Settings::path(a, &mut p, "/").unwrap();
        assert_eq!(p, q);
        p.clear();
    }
    println!(
        "{:?}",
        Settings::iter_packed()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
    );

    assert_eq!(
        Settings::path(Packed::new(0b01 << 29).unwrap(), &mut p, "/"),
        Ok(1)
    );
    assert_eq!(p, "/a");
    p.clear();
}

#[test]
fn zero_key() {
    let mut a = [[0]];
    let mut b = [[0, 0], [0, 0]];
    let mut buf = [0u8; 100];
    let mut ser = serde_json_core::ser::Serializer::new(&mut buf);
    for (n, e) in [Err(Error::TooShort(0)), Err(Error::TooShort(1)), Ok(2)]
        .iter()
        .enumerate()
    {
        assert_eq!(
            TreeSerialize::<2>::serialize_by_key(
                &mut a,
                Packed::new(0b1 << (usize::BITS - 1 - n as u32)).unwrap(),
                &mut ser
            ),
            *e
        );
        assert_eq!(
            TreeSerialize::<2>::serialize_by_key(
                &mut b,
                Packed::new(0b1 << (usize::BITS - 1 - n as u32)).unwrap(),
                &mut ser
            ),
            *e
        );
    }
}
