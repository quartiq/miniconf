use miniconf::{
    ConstPath, DescendError, FromConfig, Indices, JsonPath, KeyError, Lookup, NodeIter, Path,
    Shape, Transcode, Tree, TreeSchema,
};
mod common;

fn assert_lookup(have: Lookup, depth: usize, leaf: bool) {
    assert_eq!(have.depth, depth);
    assert_eq!(have.schema.is_leaf(), leaf);
}

#[test]
fn borrowed() {
    let mut a = "";
    miniconf::json_core::set(&mut a, "", "\"foo\"".as_bytes()).unwrap();
    assert_eq!(a, "foo");
}

#[cfg(feature = "postcard")]
#[test]
fn borrowed_u8() {
    use postcard::{de_flavors::Slice, to_slice};

    let mut a = &[0u8; 0][..];
    let mut buf = [0u8; 32];
    let data = to_slice(&[1u8, 2, 3][..], &mut buf).unwrap();
    miniconf::postcard::set_by_key(&mut a, [0; 0], Slice::new(data)).unwrap();
    assert_eq!(a, &[1, 2, 3]);
}

#[derive(Tree, Default)]
struct Inner {
    inner: f32,
}

#[derive(Tree, Default)]
struct Settings {
    a: f32,
    b: i32,
    c: Inner,
}

#[test]
fn meta() {
    const SHAPE: Shape = Settings::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 2);
    assert_eq!(SHAPE.max_length("/"), "/c/inner".len());
    assert_eq!(SHAPE.count.get(), 3);
}

#[test]
fn path() {
    assert_lookup(Settings::SCHEMA.get([1usize]).unwrap(), 1, true);
    assert_eq!(
        NodeIter::<Path<String>, 1>::with_root(Settings::SCHEMA, [1usize], '/')
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .into_inner(),
        "/b"
    );

    assert_lookup(Settings::SCHEMA.get([2usize, 0]).unwrap(), 2, true);
    assert_eq!(
        NodeIter::<Path<String>, 2>::with_root(Settings::SCHEMA, [2usize, 0], '/')
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .into_inner(),
        "/c/inner"
    );

    assert_lookup(Settings::SCHEMA.get([2usize]).unwrap(), 1, false);
    assert_lookup(Settings::SCHEMA.get([0usize; 0]).unwrap(), 0, false);
}

#[test]
fn transcode_reuse_semantics() {
    let path = Settings::SCHEMA
        .transcode::<Path<String>>([1usize])
        .unwrap();
    assert_eq!(path.as_ref(), "/b");

    let path = Path::<String>::transcode(Settings::SCHEMA, [1usize]).unwrap();
    assert_eq!(path.as_ref(), "/b");

    let path = Path::<String>::transcode_with(Settings::SCHEMA, [1usize], ':').unwrap();
    assert_eq!(path.as_ref(), ":b");

    let mut path = Path::<String>::default();
    path.transcode_from(Settings::SCHEMA, [1usize]).unwrap();
    path.transcode_from(Settings::SCHEMA, [2usize, 0]).unwrap();
    assert_eq!(path.as_ref(), "/b/c/inner");

    let mut path = ConstPath::<String, '/'>::default();
    path.transcode_from(Settings::SCHEMA, [1usize]).unwrap();
    path.transcode_from(Settings::SCHEMA, [2usize, 0]).unwrap();
    assert_eq!(path.as_ref(), "/b/c/inner");

    let mut path = JsonPath::<String>::default();
    path.transcode_from(Settings::SCHEMA, [1usize]).unwrap();
    path.transcode_from(Settings::SCHEMA, [2usize, 0]).unwrap();
    assert_eq!(path.0.as_str(), ".b.c.inner");
}

#[test]
fn indices() {
    for (keys, idx, info) in [
        ("", None, (0, false)),
        ("/b", Some(&[1][..]), (1, true)),
        ("/c/inner", Some(&[2, 0][..]), (2, true)),
        ("/c", None, (1, false)),
    ] {
        let have = Settings::SCHEMA
            .get(Path {
                path: keys,
                separator: '/',
            })
            .unwrap();
        println!("{keys} {have:?}");
        assert_lookup(have, info.0, info.1);
        if let Some(idx) = idx {
            let have = Settings::SCHEMA
                .transcode::<Indices<[usize; 2]>>(Path {
                    path: keys,
                    separator: '/',
                })
                .unwrap();
            assert_eq!(have.as_ref(), idx);
        }
    }
    assert_lookup(Option::<i8>::SCHEMA.get([0usize; 0]).unwrap(), 0, true);

    let mut it = [0usize; 4].into_iter();
    assert_eq!(
        Settings::SCHEMA.transcode::<Indices<[usize; 2]>>(&mut it),
        Err(KeyError::TooLong.into())
    );
    assert_eq!(it.count(), 2);
}

#[test]
fn get() {
    for (keys, depth, leaf) in [
        (&[][..], 0, false),
        (&[1usize][..], 1, true),
        (&[2usize][..], 1, false),
        (&[2usize, 0][..], 2, true),
    ] {
        assert_lookup(Settings::SCHEMA.get(keys).unwrap(), depth, leaf);
    }
    assert_eq!(Settings::SCHEMA.get([2usize, 0, 1]), Err(KeyError::TooLong));
    assert_eq!(Settings::SCHEMA.get(["missing"]), Err(KeyError::NotFound));
}

#[test]
fn indices_capacity() {
    let mut indices = Indices::from([0usize; 1]);
    assert_eq!(
        indices.transcode_from(Settings::SCHEMA, [2usize, 0]),
        Err(DescendError::Inner(()))
    );
    assert_eq!(indices.as_ref(), [2usize]);
    assert_eq!(indices.len(), 1);
}

#[cfg(feature = "json-core")]
#[test]
fn slice_cursor_keys() {
    let settings = Settings::default();
    let full = [2usize, 0];
    let mut rest = &full[..];
    let mut buf = [0u8; 32];
    let len = miniconf::json_core::get_by_keys(&settings, &mut rest, &mut buf).unwrap();
    assert_eq!(&buf[..len], b"0.0");
    assert_eq!(full.len() - rest.len(), 2);

    let full = [2usize, 0, 1];
    let mut rest = &full[..];
    assert_eq!(
        miniconf::json_core::get_by_keys(&settings, &mut rest, &mut buf),
        Err(miniconf::SerdeError::Value(miniconf::ValueError::Key(
            KeyError::TooLong
        )))
    );
    assert_eq!(full.len() - rest.len(), 2);
}

#[test]
fn tuple() {
    type T = (u32, (i32, u8), [u16; 3]);
    let paths = common::paths::<T, 3>();
    assert_eq!(paths.len(), 6);
    let mut s: T = Default::default();
    for p in paths {
        common::set_get(&mut s, p.as_str(), b"9");
    }
    assert_eq!(s, (9, (9, 9), [9; 3]));
}

#[test]
fn cell() {
    use core::cell::RefCell;

    let c: RefCell<i32> = Default::default();
    let mut r = &c;
    common::set_get(&mut r, "", b"9");
}
