use miniconf::{Leaf, Metadata, Node, Path, Traversal, Tree, TreeKey};

#[derive(Default)]
pub struct SkippedType;

#[derive(Tree, Default)]
struct Settings {
    #[tree(skip)]
    _long_skipped_type: SkippedType,

    value: Leaf<f32>,
}

#[test]
fn meta() {
    let meta = Settings::traverse_all::<Metadata>().unwrap();
    assert_eq!(meta.max_depth, 1);
    assert_eq!(meta.max_length("/"), "/value".len());
    assert_eq!(meta.count, 1);
}

#[test]
fn path() {
    assert_eq!(
        Settings::transcode::<Path<String, '/'>, _>([0usize]),
        Ok((Path("/value".to_owned()), Node::leaf(1)))
    );
    assert_eq!(
        Settings::transcode::<Path<String, '/'>, _>([1usize]),
        Err(Traversal::NotFound(1))
    );
}

#[test]
fn skip_struct() {
    #[allow(dead_code)]
    #[derive(Tree)]
    #[tree(flatten)]
    pub struct S(Leaf<i32>, #[tree(skip)] i32);
}
