use miniconf::{Indices, Leaf, NodeIter, Path, Tree, TreeKey};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    inner: Leaf<bool>,
}

#[derive(Tree, Default, PartialEq, Debug)]
struct Settings {
    b: [Leaf<bool>; 2],
    c: Inner,
    d: [Inner; 1],
    a: Leaf<bool>,
}

#[test]
fn struct_iter() {
    assert_eq!(
        paths::<Settings>(),
        ["/b/0", "/b/1", "/c/inner", "/d/0/inner", "/a"]
    );
}

#[test]
fn struct_iter_indices() {
    let paths = [
        ([0, 0, 0], 2),
        ([0, 1, 0], 2),
        ([1, 0, 0], 2),
        ([2, 0, 0], 3),
        ([3, 0, 0], 1),
    ];
    assert_eq!(
        Settings::nodes::<Indices<[usize; 3]>, 3>()
            .exact_size()
            .map(|have| {
                let (idx, node) = have.unwrap();
                (idx.into_inner(), node.depth())
            })
            .collect::<Vec<_>>(),
        paths
    );
}

#[test]
fn array_iter() {
    let mut s = Settings::default();

    for field in paths::<Settings>() {
        set_get(&mut s, &field, b"true");
    }

    assert!(*s.a);
    assert!(s.b.iter().all(|x| **x));
    assert!(*s.c.inner);
    assert!(s.d.iter().all(|i| *i.inner));
}

#[test]
fn short_iter() {
    assert_eq!(
        NodeIter::<Settings, Path<String, '/'>, 1>::default()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        ["/b", "/c", "/d", "/a"]
    );

    assert_eq!(
        NodeIter::<Settings, Path<String, '/'>, 0>::default()
            .next()
            .unwrap()
            .unwrap()
            .0
            .into_inner(),
        ""
    );
}

#[test]
#[should_panic]
fn panic_short_iter() {
    NodeIter::<Settings, Path<String, '/'>, 1>::default().exact_size();
}

#[test]
#[should_panic]
fn panic_started_iter() {
    let mut it = Settings::nodes::<Indices<[_; 3]>, 3>();
    it.next();
    it.exact_size();
}

#[test]
fn root() {
    assert_eq!(
        Settings::nodes::<Path<String, '/'>, 3>()
            .root(["b"])
            .unwrap()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<String>>(),
        ["/b/0", "/b/1"]
    );
}
