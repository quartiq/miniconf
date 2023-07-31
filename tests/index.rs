#![cfg(feature = "json-core")]

use miniconf::{Error, JsonCoreSlash, Miniconf};
use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Default, Miniconf, Deserialize, Serialize)]
struct Inner {
    c: u8,
}

#[derive(Debug, Default, Miniconf)]
struct Settings {
    a: [u8; 2],
    #[miniconf(defer)]
    d: [u8; 2],
    #[miniconf(defer)]
    dm: [Inner; 2],
    #[miniconf(defer(2))]
    am: miniconf::Array<Inner, 2>,
    #[miniconf(defer(3))]
    aam: miniconf::Array<miniconf::Array<Inner, 2>, 2>,
}

#[test]
fn atomic() {
    let mut s = Settings::default();
    s.set_json_by_index(&[0], b"[1,2]").unwrap();
    assert_eq!(s.a, [1, 2]);
}

#[test]
fn defer() {
    let mut s = Settings::default();
    s.set_json_by_index(&[1, 1], b"99").unwrap();
    assert_eq!(s.d, [0, 99]);
}

#[test]
fn defer_miniconf() {
    let mut s = Settings::default();
    s.set_json_by_index(&[3, 0, 0], b"1").unwrap();
    assert_eq!(s.am[0].c, 1);
    s.set_json_by_index(&[4, 0, 0, 0], b"3").unwrap();
    assert_eq!(s.aam[0][0].c, 3);
}

#[test]
fn too_short() {
    let mut s = Settings::default();
    assert_eq!(
        s.set_json_by_index(&[1], b"[1,2,3]"),
        Err(Error::TooShort(1))
    );
}

#[test]
fn too_long() {
    assert_eq!(
        Settings::default().set_json_by_index(&[0, 1], b"7"),
        Err(Error::TooLong(1))
    );
    assert_eq!(
        Settings::default().set_json_by_index(&[1, 0, 2], b"7"),
        Err(Error::TooLong(2))
    );
    assert_eq!(
        Settings::default().set_json_by_index(&[2, 0, 0], b"7"),
        Err(Error::TooLong(2))
    );
    assert_eq!(
        Settings::default().set_json_by_index(&[2, 0, 1], b"7"),
        Err(Error::TooLong(2))
    );
}

#[test]
fn not_found() {
    assert_eq!(
        Settings::default().set_json_by_index(&[1, 3], b"7"),
        Err(Error::NotFound(2))
    );
    assert_eq!(
        Settings::default().set_json_by_index(&[5], b"7"),
        Err(Error::NotFound(1))
    );
    assert_eq!(
        Settings::default().set_json_by_index(&[4, 0, 0, 1], b"7"),
        Err(Error::NotFound(4))
    );
}

#[test]
fn empty() {
    assert!([0u32; 0].set_json_by_index(&[], b"").is_err());

    #[derive(Miniconf, Serialize, Deserialize, Copy, Clone, Default)]
    struct S {}

    let mut s = [S::default(); 0];
    assert!(JsonCoreSlash::<1>::set_json_by_index(&mut s, &[], b"").is_err());

    let mut s = [[S::default(); 0]; 0];
    assert!(JsonCoreSlash::<1>::set_json_by_index(&mut s, &[], b"").is_err());

    #[derive(Miniconf, Default)]
    struct Q {
        #[miniconf(defer(2))]
        a: [S; 0],
        #[miniconf(defer)]
        b: [S; 0],
    }
    assert!(Q::default().set_json_by_index(&[], b"").is_err());
}
