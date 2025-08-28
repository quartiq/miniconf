use miniconf::{Indices, KeyError, Leaf, Metadata, Path, Schema, Tree, TreeKey};
mod common;

#[derive(Tree, Default)]
struct Inner {
    inner: Leaf<f32>,
}

#[derive(Tree, Default)]
struct Settings {
    a: Leaf<f32>,
    b: Leaf<i32>,
    c: Inner,
}

#[test]
fn meta() {
    let meta: Metadata = Settings::traverse_all();
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length("/"), "/c/inner".len());
    assert_eq!(meta.count.get(), 3);
}

#[test]
fn path() {
    for (keys, path, depth) in [
        (&[1usize][..], "/b", Schema::leaf(1)),
        (&[2, 0][..], "/c/inner", Schema::leaf(2)),
        (&[2][..], "/c", Schema::internal(1)),
        (&[][..], "", Schema::internal(0)),
    ] {
        let (s, node) = Settings::transcode::<Path<String, '/'>, _>(keys.iter()).unwrap();
        assert_eq!(node, depth);
        assert_eq!(s.as_str(), path);
    }
}

#[test]
fn indices() {
    for (keys, idx, depth) in [
        ("", [0, 0], Schema::internal(0)),
        ("/b", [1, 0], Schema::leaf(1)),
        ("/c/inner", [2, 0], Schema::leaf(2)),
        ("/c", [2, 0], Schema::internal(1)),
    ] {
        let (indices, node) =
            Settings::transcode::<Indices<_>, _>(Path::<_, '/'>::from(keys)).unwrap();
        assert_eq!(node, depth);
        assert_eq!(indices.0, idx);
    }
    let (indices, node) = Option::<Leaf<i8>>::transcode::<Indices<_>, _>([0usize; 0]).unwrap();
    assert_eq!(indices.0, [0]);
    assert_eq!(node, Schema::leaf(0));

    let mut it = [0usize; 4].into_iter();
    assert_eq!(
        Settings::transcode::<Indices<[_; 2]>, _>(&mut it),
        Err(KeyError::TooLong(1).into())
    );
    assert_eq!(it.count(), 2);
}

#[test]
fn tuple() {
    type T = (Leaf<u32>, (Leaf<i32>, Leaf<u8>), [Leaf<u16>; 3]);
    let paths = common::paths::<T, 3>();
    assert_eq!(paths.len(), 6);
    let mut s: T = Default::default();
    for p in paths {
        common::set_get(&mut s, p.as_str(), b"9");
    }
    assert_eq!(s, (9.into(), (9.into(), 9.into()), [9.into(); 3]));
}

#[test]
fn cell() {
    use core::cell::RefCell;

    let c: RefCell<Leaf<i32>> = Default::default();
    let mut r = &c;
    common::set_get(&mut r, "", b"9");
}
