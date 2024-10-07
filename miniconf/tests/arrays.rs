use miniconf::{
    json, Deserialize, Error, Indices, Metadata, Packed, Path, Serialize, Traversal, Tree, TreeKey,
};

mod common;
use common::paths;

#[derive(Debug, Copy, Clone, Default, Tree, Deserialize, Serialize)]
struct Inner {
    c: u8,
}

#[derive(Debug, Default, Tree)]
struct Settings {
    a: [u8; 2],
    #[tree(depth = 1)]
    d: [u8; 2],
    #[tree(depth = 1)]
    dm: [Inner; 2],
    #[tree(depth = 2)]
    am: [Inner; 2],
    #[tree(depth = 3)]
    aam: [[Inner; 2]; 2],
}

fn set_get(
    tree: &mut Settings,
    path: &str,
    value: &[u8],
) -> Result<usize, Error<serde_json_core::de::Error>> {
    // Path
    common::set_get(tree, path, value);

    // Indices
    let (idx, node): (Indices<[usize; 4]>, _) = Settings::transcode(&Path::<_, '/'>::from(path))?;
    assert!(node.is_leaf());
    let idx = Indices::from(&idx[..node.depth()]);
    json::set_by_key(tree, &idx, value)?;
    let mut buf = vec![0; value.len()];
    let len = json::get_by_key(tree, &idx, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);

    // Packed
    let (idx, node): (Packed, _) = Settings::transcode(&idx)?;
    assert!(node.is_leaf());
    json::set_by_key(tree, idx, value)?;
    let mut buf = vec![0; value.len()];
    let len = json::get_by_key(tree, idx, &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], value);

    Ok(node.depth())
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
        json::set(&mut s, "/d", b"[1,2]"),
        Err(Traversal::TooShort(1).into())
    );
    // Check precedence over `Inner`.
    assert_eq!(
        json::set(&mut s, "/d", b"[1,2,3]"),
        Err(Traversal::TooShort(1).into())
    );
}

#[test]
fn too_long() {
    let mut s = Settings::default();
    assert_eq!(
        json::set(&mut s, "/a/1", b"7"),
        Err(Traversal::TooLong(1).into())
    );
    assert_eq!(
        json::set(&mut s, "/d/0/b", b"7"),
        Err(Traversal::TooLong(2).into())
    );
    assert_eq!(
        json::set(&mut s, "/dm/0/c", b"7"),
        Err(Traversal::TooLong(2).into())
    );
    assert_eq!(
        json::set(&mut s, "/dm/0/d", b"7"),
        Err(Traversal::TooLong(2).into())
    );
}

#[test]
fn not_found() {
    let mut s = Settings::default();
    assert_eq!(
        json::set(&mut s, "/d/3", b"7"),
        Err(Traversal::NotFound(2).into())
    );
    assert_eq!(
        json::set(&mut s, "/b", b"7"),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        json::set(&mut s, "/aam/0/0/d", b"7"),
        Err(Traversal::NotFound(4).into())
    );
}

#[test]
fn metadata() {
    let metadata = Settings::walk::<Metadata>().unwrap();
    assert_eq!(metadata.max_depth, 4);
    assert_eq!(metadata.max_length("/"), "/aam/0/0/c".len());
    assert_eq!(metadata.count, 11);
}

#[test]
fn empty() {
    assert_eq!(paths::<[u32; 0], 1>(), [""; 0]);

    #[derive(Tree, Serialize, Deserialize)]
    struct S {}

    assert_eq!(paths::<S, 1>(), [""; 0]);
    assert_eq!(paths::<[[S; 0]; 0], 3>(), [""; 0]);

    #[derive(Tree)]
    struct Q {
        #[tree(depth = 2)]
        a: [S; 0],
        #[tree(depth = 1)]
        b: [S; 0],
    }

    assert_eq!(paths::<Q, 3>(), [""; 0]);
}
