use miniconf::{Error, JsonCoreSlash, Tree};

#[derive(Tree, Default)]
struct Inner {
    a: f32,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(validate=Self::check)]
    v: f32,
    #[tree(depth=1, validate=Inner::check)]
    i: Inner,
}

impl Settings {
    fn check(&self, new: f32, _field: &str, _old: &f32) -> Result<f32, &'static str> {
        if new.is_sign_negative() {
            Err("Must not be negative.")
        } else {
            Ok(new)
        }
    }
}

impl Inner {
    fn check(&mut self, _field: &str) -> Result<(), &'static str> {
        if self.a.is_sign_negative() {
            Err("Must not be negative.")
        } else {
            Ok(())
        }
    }
}

#[test]
fn validate() {
    let mut s = Settings::default();
    s.set_json("/v", "1.0".as_bytes()).unwrap();
    assert_eq!(s.v, 1.0);
    assert!(matches!(
        s.set_json("/v", "-1.0".as_bytes()),
        Err(Error::Invalid(1, _))
    ));
    assert_eq!(s.v, 1.0); // remains unchanged
    s.set_json("/i/a", "1.0".as_bytes()).unwrap();
    assert_eq!(s.i.a, 1.0);
    assert!(matches!(
        s.set_json("/i/a", "-1.0".as_bytes()),
        Err(Error::Invalid(1, _))
    ));
    assert_eq!(s.i.a, -1.0); // changes as validation failed at higher level
}
