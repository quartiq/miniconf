use miniconf::{Error, JsonCoreSlash, Tree};

#[derive(Tree, Default)]
struct Inner {
    a: f32,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(validate=Self::check_v)]
    v: f32,
    #[tree(depth=1, validate=Self::check_i)]
    i: Inner,
}

impl Settings {
    fn check_i(&mut self) -> Result<(), &'static str> {
        (self.i.a >= 0.0)
            .then_some(())
            .ok_or("Must not be negative.")
    }
    fn check_v(&mut self, new: f32) -> Result<f32, &'static str> {
        (new >= 0.0).then_some(new).ok_or("Must not be negative.")
    }
}

#[test]
fn validate() {
    let mut s = Settings::default();
    s.set_json("/v", "1.0".as_bytes()).unwrap();
    assert_eq!(s.v, 1.0);
    assert!(matches!(
        s.set_json("/v", "-1.0".as_bytes()),
        Err(Error::InvalidLeaf(1, _))
    ));
    assert_eq!(s.v, 1.0); // remains unchanged
    s.set_json("/i/a", "1.0".as_bytes()).unwrap();
    assert_eq!(s.i.a, 1.0);
    assert!(matches!(
        s.set_json("/i/a", "-1.0".as_bytes()),
        Err(Error::InvalidInternal(1, _))
    ));
    assert_eq!(s.i.a, -1.0); // has changed
}
