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
        paths::<3>(Settings::SCHEMA),
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
        Settings::SCHEMA
            .nodes::<Indices<[usize; 3]>, 3>()
            .exact_size()
            .map(|have| {
                let (Indices { len, data }, node) = have.unwrap();
                assert_eq!(node.depth, len);
                (data, len)
            })
            .collect::<Vec<_>>(),
        paths
    );
}

#[test]
fn array_iter() {
    let mut s = Settings::default();

    for field in paths::<3>(Settings::SCHEMA) {
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
        NodeIter::<Path<String, '/'>, 1>::new(Settings::SCHEMA)
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        ["/b", "/c", "/d", "/a"]
    );

    assert_eq!(
        NodeIter::<Path<String, '/'>, 0>::new(Settings::SCHEMA)
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
    <[[Leaf<u32>; 1]; 1]>::SCHEMA.nodes::<(), 1>().exact_size();
}

#[test]
#[should_panic]
fn panic_started_iter() {
    let mut it = <[[Leaf<u32>; 1]; 1]>::SCHEMA.nodes::<(), 2>();
    it.next();
    it.exact_size();
}

#[test]
#[should_panic]
fn panic_rooted_iter() {
    <[[Leaf<u32>; 1]; 1]>::SCHEMA
        .nodes::<(), 2>()
        .root([0usize])
        .unwrap()
        .exact_size();
}

#[test]
fn root() {
    assert_eq!(
        Settings::SCHEMA
            .nodes::<Path<String, '/'>, 3>()
            .root(["b"])
            .unwrap()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<String>>(),
        ["/b/0", "/b/1"]
    );
}
