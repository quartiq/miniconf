use miniconf::{Traversal, Tree, TreeKey};

#[derive(Tree, Default)]
struct Inner {
    inner: f32,
}

#[derive(Tree, Default)]
struct Settings {
    a: f32,
    b: i32,
    #[tree(depth = 1)]
    c: Inner,
}

#[test]
fn meta() {
    let meta = Settings::metadata();
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length("/"), "/c/inner".len());
    assert_eq!(meta.count, 3);
}

#[test]
fn path() {
    let mut s = String::new();
    assert_eq!(Settings::path([1], &mut s, "/"), Ok(1));
    assert_eq!(s, "/b");
    s.clear();
    assert_eq!(Settings::path([2, 0], &mut s, "/"), Ok(2));
    assert_eq!(s, "/c/inner");
    s.clear();
    assert_eq!(Settings::path([2], &mut s, "/"), Ok(1));
    assert_eq!(s, "/c");
    s.clear();
    assert_eq!(Option::<i8>::path([0; 0], &mut s, "/"), Ok(0));
    assert_eq!(s, "");
}

#[test]
fn indices() {
    assert_eq!(Settings::indices(["b"]), Ok(([1, 0], 1)));
    assert_eq!(Settings::indices(["c", "inner"]), Ok(([2, 0], 2)));
    assert_eq!(Settings::indices(["c"]), Ok(([2, 0], 1)));
    assert_eq!(Option::<i8>::indices([0; 0]), Ok(([0], 0)));
}

#[test]
fn traverse_empty() {
    #[derive(Tree, Default)]
    struct S {}
    let f = |_, _, _| -> Result<(), ()> { unreachable!() };
    assert_eq!(
        S::traverse_by_key([0].into_iter(), f),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        S::traverse_by_key([0; 0].into_iter(), f),
        Err(Traversal::TooShort(0).into())
    );
    assert_eq!(Option::<i32>::traverse_by_key([0].into_iter(), f), Ok(0));
    assert_eq!(Option::<i32>::traverse_by_key([0; 0].into_iter(), f), Ok(0));
    assert_eq!(
        <Option::<S> as TreeKey<2>>::traverse_by_key([0].into_iter(), f),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(
        <Option::<S> as TreeKey<2>>::traverse_by_key([0; 0].into_iter(), f),
        Err(Traversal::TooShort(0).into())
    );
}
