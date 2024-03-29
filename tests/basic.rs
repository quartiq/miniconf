use miniconf::{Error, Tree, TreeKey};

#[derive(Tree, Default)]
struct Inner {
    inner: f32,
}

#[derive(Tree, Default)]
struct Settings {
    a: f32,
    b: i32,
    #[tree()]
    c: Inner,
}

#[test]
fn meta() {
    let meta = Settings::metadata().separator("/");
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "/c/inner".len());
    assert_eq!(meta.count, 3);
}

#[test]
fn path() {
    let mut s = String::new();
    assert_eq!(Settings::path([1, 0, 0], &mut s, "/"), Ok(1));
    assert_eq!(s, "/b");
    s.clear();
    assert_eq!(Settings::path([2, 0, 0], &mut s, "/"), Ok(2));
    assert_eq!(s, "/c/inner");
    s.clear();
    assert_eq!(Settings::path([2], &mut s, "/"), Err(Error::TooShort(1)));
    assert_eq!(s, "/c");
    s.clear();
    assert_eq!(Option::<i8>::path([0; 0], &mut s, "/"), Ok(0));
    assert_eq!(s, "");
}

#[test]
fn indices() {
    let mut s = [0usize; 2];
    assert_eq!(Settings::indices(["b", "foo"], s.iter_mut()), Ok(1));
    assert_eq!(s, [1, 0]);
    assert_eq!(
        Settings::indices(["c", "inner", "bar"], s.iter_mut()),
        Ok(2)
    );
    assert_eq!(s, [2, 0]);
    assert_eq!(
        Settings::indices(["c"], s.iter_mut()),
        Err(Error::TooShort(1))
    );
    assert_eq!(Option::<i8>::indices([0; 0], s.iter_mut()), Ok(0));
}

#[test]
fn traverse_empty() {
    #[derive(Tree, Default)]
    struct S {}
    let f = |_, _: &_| unreachable!();
    assert_eq!(
        S::traverse_by_key([0].into_iter(), f),
        Err(Error::<()>::NotFound(1))
    );
    assert_eq!(
        S::traverse_by_key([0; 0].into_iter(), f),
        Err(Error::TooShort(0))
    );
    assert_eq!(Option::<i32>::traverse_by_key([0].into_iter(), f), Ok(0));
    assert_eq!(
        <Option::<S> as TreeKey<2>>::traverse_by_key([0].into_iter(), f),
        Err(Error::NotFound(1))
    );
    assert_eq!(
        <Option::<S> as TreeKey<2>>::traverse_by_key([0; 0].into_iter(), f),
        Err(Error::TooShort(0))
    );
}
