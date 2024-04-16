#![cfg(feature = "json-core")]
use miniconf::{Error, JsonCoreSlash, Tree};

#[derive(Tree, Default)]
struct Inner {
    a: f32,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(getter=Self::v, setter=Self::set_v)]
    v: f32,
    #[tree(depth=1, getter=Self::i, setter=Self::set_i)]
    i: Inner,
}

impl Settings {
    fn i(&self) -> Result<&Inner, &'static str> {
        Ok(&self.i)
    }

    fn set_i(&mut self) -> Result<(), &'static str> {
        if self.i.a >= 0.0 {
            Ok(())
        } else {
            Err("Must not be negative.")
        }
    }

    fn v(&self) -> Result<&f32, &'static str> {
        Ok(&self.v)
    }

    fn set_v(&mut self, new: f32) -> Result<(), &'static str> {
        if new >= 0.0 {
            self.v = new;
            Ok(())
        } else {
            Err("Must not be negative.")
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
