use miniconf::{JsonCoreSlash, Tree};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: i32,
}

#[test]
fn struct_flatten() {
    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S1 {
        a: i32,
    }
    assert_eq!(paths::<S1, 1>(), [""]);
    let mut s = S1::default();
    set_get(&mut s, "", b"1");
    assert_eq!(s.a, 1);

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S2(i32);
    assert_eq!(paths::<S2, 1>(), [""]);
    let mut s = S2::default();
    set_get(&mut s, "", b"1");
    assert_eq!(s.0, 1);

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S3(#[tree(depth = 1)] Inner);
    assert_eq!(paths::<S3, 1>(), ["/a"]);
    let mut s = S3::default();
    set_get(&mut s, "/a", b"1");
    assert_eq!(s.0.a, 1);
}

#[test]
fn enum_flatten() {
    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    enum E1 {
        #[default]
        None,
        A(i32),
    }
    assert_eq!(paths::<E1, 1>(), [""]);
    let mut e = E1::A(0);
    set_get(&mut e, "", b"1");
    assert_eq!(e, E1::A(1));
    assert_eq!(
        E1::None.set_json("", b"1").unwrap_err(),
        miniconf::Traversal::Absent(0).into()
    );

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    enum E2 {
        #[default]
        None,
        A(#[tree(depth = 1)] Inner),
    }
    assert_eq!(paths::<E2, 1>(), ["/a"]);
    let mut e = E2::A(Inner::default());
    set_get(&mut e, "/a", b"1");
    assert_eq!(e, E2::A(Inner { a: 1 }));
    assert_eq!(
        E2::None.set_json("/a", b"1").unwrap_err(),
        miniconf::Traversal::Absent(0).into()
    );
}
