#![cfg(feature = "json-core")]

use miniconf::{Deserialize, JsonCoreSlash, Serialize, Traversal, Tree};

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

#[test]
fn atomic() {
    let mut s = Settings::default();
    s.set_json_by_key([0], b"[1,2]").unwrap();
    assert_eq!(s.a, [1, 2]);
}

#[test]
fn defer() {
    let mut s = Settings::default();
    s.set_json_by_key([1, 1], b"99").unwrap();
    assert_eq!(s.d, [0, 99]);
}

#[test]
fn defer_miniconf() {
    let mut s = Settings::default();
    s.set_json_by_key([3, 0, 0], b"1").unwrap();
    assert_eq!(s.am[0].c, 1);
    s.set_json_by_key([4, 0, 0, 0], b"3").unwrap();
    assert_eq!(s.aam[0][0].c, 3);
}

#[test]
fn too_short() {
    let mut s = Settings::default();
    assert_eq!(
        s.set_json_by_key([1], b"[1,2,3]"),
        Err(Traversal::TooShort(1).into())
    );
}

#[test]
fn too_long() {
    assert_eq!(
        Settings::default().set_json_by_key([0, 1], b"7"),
        Err(Traversal::TooLong(1).into())
    );
    assert_eq!(
        Settings::default().set_json_by_key([1, 0, 2], b"7"),
        Err(Traversal::TooLong(2).into())
    );
    assert_eq!(
        Settings::default().set_json_by_key([2, 0, 0], b"7"),
        Err(Traversal::TooLong(2).into())
    );
    assert_eq!(
        Settings::default().set_json_by_key([2, 0, 1], b"7"),
        Err(Traversal::TooLong(2).into())
    );
}

#[test]
fn not_found() {
    assert_eq!(
        Settings::default().set_json_by_key([1, 3], b"7"),
        Err(Traversal::NotFound(2).into())
    );
    assert_eq!(
        Settings::default().set_json_by_key([5], b"7"),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        Settings::default().set_json_by_key([4, 0, 0, 1], b"7"),
        Err(Traversal::NotFound(4).into())
    );
}

#[test]
fn empty() {
    assert!([0u32; 0].set_json_by_key([0usize; 0], b"").is_err());

    #[derive(Tree, Serialize, Deserialize, Copy, Clone, Default)]
    struct S {}

    let mut s = [S::default(); 0];
    assert!(JsonCoreSlash::<1>::set_json_by_key(&mut s, [0usize; 0], b"1").is_err());

    let mut s = [[S::default(); 0]; 0];
    assert!(JsonCoreSlash::<1>::set_json_by_key(&mut s, [0usize; 0], b"1").is_err());

    #[derive(Tree, Default)]
    struct Q {
        #[tree(depth = 2)]
        a: [S; 0],
        #[tree(depth = 1)]
        b: [S; 0],
    }
    assert!(Q::default().set_json_by_key([0usize; 0], b"").is_err());
}
