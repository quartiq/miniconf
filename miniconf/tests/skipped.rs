use miniconf::{KeyError, Leaf, Path, Shape, Tree, TreeKey};

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
    let meta: Shape = Settings::SCHEMA.shape();
    assert_eq!(meta.max_depth, 1);
    assert_eq!(meta.max_length("/"), "/value".len());
    assert_eq!(meta.count.get(), 1);
}

#[test]
fn path() {
    assert_eq!(
        Settings::SCHEMA.transcode::<Path<String, '/'>>([0usize]),
        Ok(Path("/value".to_owned()))
    );
    assert_eq!(
        Settings::SCHEMA.transcode::<Path<String, '/'>>([1usize]),
        Err(KeyError::NotFound.into())
    );
}

#[test]
fn skip_struct() {
    #[allow(dead_code)]
    #[derive(Tree)]
    #[tree(flatten)]
    pub struct S(Leaf<i32>, #[tree(skip)] i32);
}
