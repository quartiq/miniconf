use miniconf::{Indices, KeyError, Leaf, Path, Shape, Short, Track, Tree, TreeSchema};
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
    let meta: Shape = Settings::SCHEMA.shape();
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length("/"), "/c/inner".len());
    assert_eq!(meta.count.get(), 3);
}

#[test]
fn path() {
    for (keys, path, depth, leaf) in [
        (&[1usize][..], "/b", 1, true),
        (&[2, 0][..], "/c/inner", 2, true),
        (&[2][..], "/c", 1, false),
        (&[][..], "", 0, false),
    ] {
        let s = Settings::SCHEMA
            .transcode::<Short<Track<Path<String, '/'>>>>(keys)
            .unwrap();
        assert_eq!(depth, s.inner.depth);
        assert_eq!(leaf, s.leaf);
        assert_eq!(s.inner.inner.as_str(), path);
    }
}

#[test]
fn indices() {
    for (keys, idx, depth, leaf) in [
        ("", [0, 0], 0, false),
        ("/b", [1, 0], 1, true),
        ("/c/inner", [2, 0], 2, true),
        ("/c", [2, 0], 1, false),
    ] {
        let indices = Settings::SCHEMA
            .transcode::<Short<Indices<_>>>(Path::<_, '/'>::from(keys))
            .unwrap();
        assert_eq!(indices.inner.len, depth);
        assert_eq!(indices.leaf, leaf);
        assert_eq!(indices.inner.data, idx);
    }
    let indices = Option::<Leaf<i8>>::SCHEMA
        .transcode::<Short<Indices<_>>>([0usize; 0])
        .unwrap();
    assert_eq!(indices.inner.data, [0]);
    assert_eq!(indices.leaf, true);
    assert_eq!(indices.inner.len, 0);

    let mut it = [0usize; 4].into_iter();
    assert_eq!(
        Settings::SCHEMA.transcode::<Indices<[_; 2]>>(&mut it),
        Err(KeyError::TooLong.into())
    );
    assert_eq!(it.count(), 2);
}

#[test]
fn tuple() {
    type T = (Leaf<u32>, (Leaf<i32>, Leaf<u8>), [Leaf<u16>; 3]);
    let paths = common::paths::<3>(T::SCHEMA);
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
