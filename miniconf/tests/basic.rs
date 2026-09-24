#[cfg(feature = "postcard")]
use miniconf::postcard as minipostcard;
use miniconf::{
    ConstPath, DescendError, Indices, Internal, JsonPath, KeyError, Lookup, Meta, NodeIter, Path,
    SerdeError, Shape, Transcode, Tree, TreeSchema, Ty, ValueError, json_core, str_leaf,
};
mod common;

#[test]
fn derive_inside_macro() {
    macro_rules! trees {
        ($field:ident, $ty:ty) => {
            #[derive(Tree)]
            struct Named {
                $field: $ty,
            }
            #[derive(Tree)]
            struct Tuple($ty);
        };
    }
    trees!(value, u8);
    let mut named = Named { value: 0 };
    let mut tuple = Tuple(0);
    json_core::set(&mut named, "/value", b"7").unwrap();
    json_core::set(&mut tuple, "/0", b"9").unwrap();
    assert_eq!((named.value, tuple.0), (7, 9));
}

#[test]
fn dynamic_keys() {
    use miniconf::{IntoKeys, Keys};
    let mut tree = [[0u32; 2]; 2];
    let mut path = "/1/0".into_keys();
    let mut indices = [1usize, 0].as_slice();
    for keys in [&mut path as &mut dyn Keys, &mut indices] {
        json_core::set_by_keys(&mut tree, keys, b"17").unwrap();
        assert_eq!(tree[1][0], 17);
        tree[1][0] = 0;
    }
    let mut buffer = [0; 8];
    let len = json_core::get_by_keys(&tree, &mut "/1/0".into_keys() as &mut dyn Keys, &mut buffer)
        .unwrap();
    assert_eq!(&buffer[..len], b"0");
    assert_eq!(
        json_core::get_by_keys(
            &tree,
            &mut "/1/0/0".into_keys() as &mut dyn Keys,
            &mut buffer
        ),
        Err(SerdeError::from(KeyError::TooLong))
    );
}

fn assert_lookup(have: Lookup, depth: usize, leaf: bool) {
    assert_eq!(have.depth, depth);
    assert_eq!(have.schema.is_leaf(), leaf);
}

#[test]
fn borrowed() {
    let mut a = "";
    json_core::set(&mut a, "", "\"foo\"".as_bytes()).unwrap();
    assert_eq!(a, "foo");
}

#[cfg(feature = "postcard")]
#[test]
fn borrowed_u8() {
    use postcard::{de_flavors::Slice, to_slice};

    let mut a = &[0u8; 0][..];
    let mut buf = [0u8; 32];
    let data = to_slice(&[1u8, 2, 3][..], &mut buf).unwrap();
    minipostcard::set_by_key(&mut a, [0; 0], Slice::new(data)).unwrap();
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
        NodeIter::<Path<String>, 1>::with_root(Settings::SCHEMA, [1usize])
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .into_inner(),
        "/b"
    );

    assert_lookup(Settings::SCHEMA.get([2usize, 0]).unwrap(), 2, true);
    assert_eq!(
        NodeIter::<Path<String>, 2>::with_root(Settings::SCHEMA, [2usize, 0])
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

    let mut path = Path::new(String::new(), ':');
    path.transcode_from(Settings::SCHEMA, &[1usize][..])
        .unwrap();
    assert_eq!(path.as_ref(), ":b");

    let mut path = Path::<String>::default();
    path.transcode_from(Settings::SCHEMA, &[1usize][..])
        .unwrap();
    path.transcode_from(Settings::SCHEMA, &[2usize, 0][..])
        .unwrap();
    assert_eq!(path.as_ref(), "/b/c/inner");

    let mut path = ConstPath::<String, '/'>::default();
    path.transcode_from(Settings::SCHEMA, &[1usize][..])
        .unwrap();
    path.transcode_from(Settings::SCHEMA, &[2usize, 0][..])
        .unwrap();
    assert_eq!(path.as_ref(), "/b/c/inner");

    let mut path = JsonPath::<String>::default();
    path.transcode_from(Settings::SCHEMA, &[1usize][..])
        .unwrap();
    path.transcode_from(Settings::SCHEMA, &[2usize, 0][..])
        .unwrap();
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
        let have = Settings::SCHEMA.get(keys).unwrap();
        println!("{keys} {have:?}");
        assert_lookup(have, info.0, info.1);
        if let Some(idx) = idx {
            let have = Settings::SCHEMA
                .transcode::<Indices<[usize; 2]>>(keys)
                .unwrap();
            assert_eq!(have.as_ref(), idx);
        }
    }
    assert_lookup(Option::<i8>::SCHEMA.get([0usize; 0]).unwrap(), 0, true);

    assert_eq!(
        Settings::SCHEMA.transcode::<Indices<[usize; 2]>>([0usize, 0, 0, 0]),
        Err(KeyError::TooLong.into())
    );
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
        indices.transcode_from(Settings::SCHEMA, &[2usize, 0][..]),
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
    let len = json_core::get_by_keys(&settings, &mut rest, &mut buf).unwrap();
    assert_eq!(&buf[..len], b"0.0");
    assert_eq!(full.len() - rest.len(), 2);

    let full = [2usize, 0, 1];
    let mut rest = &full[..];
    assert_eq!(
        json_core::get_by_keys(&settings, &mut rest, &mut buf),
        Err(SerdeError::Value(ValueError::Key(KeyError::TooLong)))
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

#[test]
fn meta_option_is_niche_optimized() {
    assert_eq!(
        core::mem::size_of::<Option<Meta>>(),
        core::mem::size_of::<Meta>()
    );
}

#[cfg(feature = "sem")]
#[test]
fn builtin_oneof_sem() {
    let schema = Result::<u32, i32>::SCHEMA;
    assert!(schema.sem().unwrap().oneof());
    let Internal::Named(children) = schema.internal().unwrap() else {
        panic!("expected named internal schema");
    };
    assert_eq!(children[0].name(), "Ok");
    assert_eq!(children[1].name(), "Err");

    let schema = core::ops::Bound::<u32>::SCHEMA;
    assert!(schema.sem().unwrap().oneof());
    let Internal::Named(children) = schema.internal().unwrap() else {
        panic!("expected named internal schema");
    };
    assert_eq!(children[0].name(), "Included");
    assert_eq!(children[1].name(), "Excluded");
}

#[cfg(all(feature = "sem", feature = "std"))]
#[test]
fn string_like_leaf_ty() {
    assert_eq!(
        std::net::SocketAddr::SCHEMA.sem().unwrap().ty(),
        Some(Ty::Str)
    );
    assert_eq!(
        std::path::PathBuf::SCHEMA.sem().unwrap().ty(),
        Some(Ty::Str)
    );
}

#[cfg(feature = "sem")]
#[test]
fn builtin_ty_sem() {
    assert_eq!(u32::SCHEMA.sem().unwrap().ty(), Some(Ty::U32));
    assert_eq!(str_leaf::SCHEMA.sem().unwrap().ty(), Some(Ty::Str));
}

#[test]
fn lookup_preserves_finalize_errors() {
    use miniconf::{IntoKeys, Keys};

    struct Cursor(KeyError);
    impl Keys for Cursor {
        fn next(&mut self, _: &Internal) -> Result<usize, KeyError> {
            Ok(0)
        }
        fn finalize(&mut self) -> Result<(), KeyError> {
            Err(self.0)
        }
    }
    impl IntoKeys for Cursor {
        type IntoKeys = Self;
        fn into_keys(self) -> Self {
            self
        }
    }
    for error in [KeyError::NotFound, KeyError::TooShort, KeyError::TooLong] {
        let schema = <[u8; 1]>::SCHEMA;
        assert_eq!(schema.get(Cursor(error)), Err(error));
        let mut state = [usize::MAX];
        let failure = schema.resolve_into(Cursor(error), &mut state).unwrap_err();
        assert_eq!(failure.error, DescendError::Key(error));
        assert_eq!(failure.lookup.depth, 1);
        assert!(failure.lookup.schema.is_leaf());
        assert_eq!(state, [0]);
    }
}
