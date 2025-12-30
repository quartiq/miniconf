use miniconf::{Indices, NodeIter, Path, Short, Tree, TreeSchema};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    inner: bool,
}

#[derive(Tree, Default, PartialEq, Debug)]
struct Settings {
    b: [bool; 2],
    c: Inner,
    d: [Inner; 1],
    a: bool,
}

#[test]
fn struct_iter() {
    assert_eq!(
        paths::<Settings, 3>(),
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
            .map(|have| have.unwrap().into_inner())
            .collect::<Vec<_>>(),
        paths
    );
}

#[test]
fn array_iter() {
    let mut s = Settings::default();

    for field in paths::<Settings, 3>() {
        set_get(&mut s, &field, b"true");
    }

    assert!(s.a);
    assert!(s.b.iter().all(|x| *x));
    assert!(s.c.inner);
    assert!(s.d.iter().all(|i| i.inner));
}

#[test]
fn short_iter() {
    assert_eq!(
        NodeIter::<Short<Path<String, '/'>>, 1>::new(Settings::SCHEMA)
            .map(|p| p.unwrap().into_inner().0.into_inner())
            .collect::<Vec<_>>(),
        ["/b", "/c", "/d", "/a"]
    );

    assert_eq!(
        NodeIter::<Short<Path<String, '/'>>, 0>::new(Settings::SCHEMA)
            .next()
            .unwrap()
            .unwrap()
            .into_inner()
            .0
            .into_inner(),
        ""
    );
}

#[test]
#[should_panic]
fn panic_short_iter() {
    <[(); 1]>::SCHEMA.nodes::<(), 0>();
    // compile time:
    // <[(); 1]>::nodes::<(), 0>();
}

#[test]
fn root() {
    assert_eq!(
        NodeIter::<Path<String, '/'>, 3>::with_root(Settings::SCHEMA, ["b"])
            .unwrap()
            .map(|p| p.unwrap().into_inner())
            .collect::<Vec<_>>(),
        ["/b/0", "/b/1"]
    );
}
