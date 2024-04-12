use miniconf::{Error, JsonCoreSlash, Tree};

#[derive(Tree, Default)]
struct Inner {
    a: f32,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(validate=Self::check_v)]
    v: f32,
    #[tree(validate=Self::check_i, depth=1)]
    i: Inner,
}

impl Settings {
    fn check_v(&self, _field: &str, new: &mut f32, _old: &f32) -> Result<(), &'static str> {
        if new.is_sign_negative() {
            Err("Must not be negative.")
        } else {
            Ok(())
        }
    }
    fn check_i(_field: &str, _new: &mut Inner) -> Result<(), &'static str> {
        Ok(())
    }
}

#[test]
fn validate() {
    let mut s = Settings::default();
    s.set_json("/v", "1.0".as_bytes()).unwrap();
    assert_eq!(s.v, 1.0);
    s.set_json("/v", "-1.0".as_bytes()).unwrap_err();
    assert_eq!(s.v, 1.0);
    s.set_json("/i/a", "1.0".as_bytes()).unwrap();
    assert_eq!(s.i.a, 1.0);
}
