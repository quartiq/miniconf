use miniconf::{json, Leaf, Tree};

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
    assert_eq!(paths::<S1, 1>(), [""]);
    let mut s = S1::default();
    set_get(&mut s, "", b"1");
    assert_eq!(*s.a, 1);

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S2(Leaf<i32>);
    assert_eq!(paths::<S2, 1>(), [""]);
    let mut s = S2::default();
    set_get(&mut s, "", b"1");
    assert_eq!(*s.0, 1);

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    struct S3(Inner);
    assert_eq!(paths::<S3, 1>(), ["/a"]);
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
    assert_eq!(paths::<E1, 1>(), [""]);
    let mut e = E1::A(Leaf(0));
    set_get(&mut e, "", b"1");
    assert_eq!(e, E1::A(1.into()));
    assert_eq!(
        json::set(&mut E1::None, "", b"1").unwrap_err(),
        miniconf::Traversal::Absent(0).into()
    );

    #[derive(Tree, Default, PartialEq, Debug)]
    #[tree(flatten)]
    enum E2 {
        #[default]
        None,
        A(Inner),
    }
    assert_eq!(paths::<E2, 1>(), ["/a"]);
    let mut e = E2::A(Inner::default());
    set_get(&mut e, "/a", b"1");
    assert_eq!(e, E2::A(Inner { a: Leaf(1) }));
    assert_eq!(
        json::set(&mut E2::None, "/a", b"1").unwrap_err(),
        miniconf::Traversal::Absent(0).into()
    );
}
