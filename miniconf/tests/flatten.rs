use miniconf::{json, Leaf, Tree, TreeSchema};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: Leaf<i32>,
}

#[test]
fn struct_flatten() {
    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S1 {
        a: Leaf<i32>,
    }
    assert_eq!(paths::<1>(S1::SCHEMA), [""]);
    let mut s = S1::default();
    set_get(&mut s, "", b"1");
    assert_eq!(*s.a, 1);

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S2(Leaf<i32>);
    assert_eq!(paths::<1>(S2::SCHEMA), [""]);
    let mut s = S2::default();
    set_get(&mut s, "", b"1");
    assert_eq!(*s.0, 1);

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S3(Inner);
    assert_eq!(paths::<1>(S3::SCHEMA), ["/a"]);
    let mut s = S3::default();
    set_get(&mut s, "/a", b"1");
    assert_eq!(*s.0.a, 1);
}

#[test]
fn enum_flatten() {
    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    enum E1 {
        #[default]
        None,
        A(Leaf<i32>),
    }
    assert_eq!(paths::<1>(E1::SCHEMA), [""]);
    let mut e = E1::A(Leaf(0));
    set_get(&mut e, "", b"1");
    assert_eq!(e, E1::A(1.into()));
    assert_eq!(
        json::set(&mut E1::None, "", b"1").unwrap_err(),
        miniconf::ValueError::Absent.into()
    );

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    enum E2 {
        #[default]
        None,
        A(Inner),
    }
    assert_eq!(paths::<1>(E2::SCHEMA), ["/a"]);
    let mut e = E2::A(Inner::default());
    set_get(&mut e, "/a", b"1");
    assert_eq!(e, E2::A(Inner { a: Leaf(1) }));
    assert_eq!(
        json::set(&mut E2::None, "/a", b"1").unwrap_err(),
        miniconf::ValueError::Absent.into()
    );
}
