use miniconf::{
    Deserialize, Indices, KeyError, Leaf, Packed, Path, SerdeError, Serialize, Shape, Track, Tree,
    TreeSchema, json_core, leaf,
};

mod common;

#[derive(Debug, Copy, Clone, Default, Tree, Deserialize, Serialize)]
struct Inner {
    c: u8,
}

#[derive(Debug, Default, Tree)]
struct Settings {
    #[tree(with=leaf)]
    a: [u8; 2],
    d: [u8; 2],
    dm: [Leaf<Inner>; 2],
    am: [Inner; 2],
    aam: [[Inner; 2]; 2],
}

fn set_get(
    tree: &mut Settings,
    path: &str,
    value: &[u8],
) -> Result<usize, SerdeError<serde_json_core::de::Error>> {
    // Path
    common::set_get(tree, path, value);

    // Indices
    let (idx, depth) = Settings::SCHEMA
        .transcode::<Track<Indices<[usize; 4]>>>(Path::<_, '/'>(path))
        .unwrap()
        .into_inner();

    assert_eq!(depth, idx.len());
    json_core::set_by_key(tree, &idx, value)?;
    let mut buf = vec![0; value.len()];
    let len = json_core::get_by_key(tree, &idx, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);

    // Packed
    let (packed, depth) = Settings::SCHEMA
        .transcode::<Track<Packed>>(&idx)
        .unwrap()
        .into_inner();
    assert_eq!(depth, idx.len());
    json_core::set_by_key(tree, packed, value)?;
    let mut buf = vec![0; value.len()];
    let len = json_core::get_by_key(tree, packed, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);

    Ok(depth)
}

#[test]
fn paths() {
    common::paths::<Settings, 4>();
}

#[test]
fn atomic() {
    let mut s = Settings::default();
    set_get(&mut s, "/a", b"[1,2]").unwrap();
    assert_eq!(s.a, [1, 2]);
}

#[test]
fn defer() {
    let mut s = Settings::default();
    set_get(&mut s, "/d/1", b"99").unwrap();
    assert_eq!(s.d[1], 99);
}

#[test]
fn defer_miniconf() {
    let mut s = Settings::default();
    set_get(&mut s, "/am/0/c", b"1").unwrap();
    assert_eq!(s.am[0].c, 1);
    set_get(&mut s, "/aam/0/0/c", b"3").unwrap();
    assert_eq!(s.aam[0][0].c, 3);
}

#[test]
fn too_short() {
    let mut s = Settings::default();
    assert_eq!(
        json_core::set(&mut s, "/d", b"[1,2]"),
        Err(KeyError::TooShort.into())
    );
    // Check precedence over `Inner`.
    assert_eq!(
        json_core::set(&mut s, "/d", b"[1,2,3]"),
        Err(KeyError::TooShort.into())
    );
}

#[test]
fn too_long() {
    let mut s = Settings::default();
    assert_eq!(
        json_core::set(&mut s, "/a/1", b"7"),
        Err(KeyError::TooLong.into())
    );
    assert_eq!(
        json_core::set(&mut s, "/d/0/b", b"7"),
        Err(KeyError::TooLong.into())
    );
    assert_eq!(
        json_core::set(&mut s, "/dm/0/c", b"7"),
        Err(KeyError::TooLong.into())
    );
    assert_eq!(
        json_core::set(&mut s, "/dm/0/d", b"7"),
        Err(KeyError::TooLong.into())
    );
}

#[test]
fn not_found() {
    let mut s = Settings::default();
    assert_eq!(
        json_core::set(&mut s, "/d/3", b"7"),
        Err(KeyError::NotFound.into())
    );
    assert_eq!(
        json_core::set(&mut s, "/b", b"7"),
        Err(KeyError::NotFound.into())
    );
    assert_eq!(
        json_core::set(&mut s, "/aam/0/0/d", b"7"),
        Err(KeyError::NotFound.into())
    );
}

#[test]
fn metadata() {
    const SHAPE: Shape = Settings::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 4);
    assert_eq!(SHAPE.max_length("/"), "/aam/0/0/c".len());
    assert_eq!(SHAPE.count.get(), 11);
}
