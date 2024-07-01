use miniconf::{Node, Path, Traversal, Tree, TreeKey};

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
    let meta = Settings::metadata();
    assert_eq!(meta.max_depth, 1);
    assert_eq!(meta.max_length("/"), "/value".len());
    assert_eq!(meta.count, 1);
}

#[test]
fn path() {
    assert_eq!(
        Settings::transcode::<Path<String, '/'>, _>([0]),
        Ok((Path("/value".to_owned()), Node::leaf(1)))
    );
    assert_eq!(
        Settings::transcode::<Path<String, '/'>, _>([1]),
        Err(Traversal::NotFound(1))
    );
}
