use miniconf::{Traversal, Tree, TreeKey};

#[derive(Default)]
pub struct SkippedType;

#[derive(Tree, Default)]
struct Settings {
    #[tree(skip)]
    _long_skipped_type: SkippedType,

    value: f32,
}

#[test]
fn meta() {
    let meta = Settings::metadata().separator("/");
    assert_eq!(meta.max_depth, 1);
    assert_eq!(meta.max_length, "/value".len());
    assert_eq!(meta.count, 1);
}

#[test]
fn path() {
    let mut s = String::new();
    assert_eq!(Settings::path([0], &mut s, "/"), Ok(1));
    assert_eq!(s, "/value");
    s.clear();
    assert_eq!(
        Settings::path([1], &mut s, "/"),
        Err(Traversal::NotFound(1).into())
    );
    assert_eq!(s, "");
}
