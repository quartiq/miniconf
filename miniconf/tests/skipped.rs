use miniconf::{KeyError, Path, Shape, Tree, TreeSchema};

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
    const SHAPE: Shape = Settings::SCHEMA.shape();
    assert_eq!(SHAPE.max_depth, 1);
    assert_eq!(SHAPE.max_length("/"), "/value".len());
    assert_eq!(SHAPE.count.get(), 1);
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
    pub struct S(i32, #[tree(skip)] i32);
}
