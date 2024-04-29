#![cfg(all(feature = "json-core", feature = "derive"))]

use miniconf::{JsonCoreSlash, Traversal, Tree};

#[derive(Tree, Default)]
struct Inner {
    a: f32,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(validate=Self::validate_v)]
    v: f32,
    #[tree(depth=1, validate=Self::validate_i)]
    i: Inner,
}

impl Settings {
    fn validate_v(&mut self, new: f32) -> Result<f32, &'static str> {
        if new >= 0.0 {
            Ok(new)
        } else {
            Err("")
        }
    }

    fn validate_i(&mut self, depth: usize) -> Result<usize, &'static str> {
        if self.i.a >= 0.0 {
            Ok(depth)
        } else {
            Err("")
        }
    }
}

#[test]
fn validate() {
    let mut s = Settings::default();
    s.set_json("/v", b"1.0").unwrap();
    assert_eq!(s.v, 1.0);
    assert_eq!(
        s.set_json("/v", b"-1.0"),
        Err(Traversal::Invalid(1, "").into())
    );
    assert_eq!(s.v, 1.0); // remains unchanged
    s.set_json("/i/a", b"1.0").unwrap();
    assert_eq!(s.i.a, 1.0);
    assert_eq!(
        s.set_json("/i/a", b"-1.0"),
        Err(Traversal::Invalid(1, "").into())
    );
    assert_eq!(s.i.a, -1.0); // has changed as internal validation was done after leaf setting
    assert_eq!(s.set_json("/i/a", b"1.0"), Ok(3));
}

#[test]
fn other_type() {
    // Demonstrate and test how a variable length `Vec` can be accessed
    // through a variable offset, fixed length array.
    #[derive(Default, Tree)]
    struct S {
        #[tree(depth=1, typ="[i32; 4]", get=Self::get::<4>, get_mut=Self::get_mut::<4>, rename=arr)]
        vec: Vec<i32>,
        offset: usize,
    }
    impl S {
        fn get<const N: usize>(&self) -> Result<&[i32; N], &'static str> {
            Ok(self
                .vec
                .get(self.offset..self.offset + N)
                .ok_or("range")?
                .try_into()
                .unwrap())
        }
        fn get_mut<const N: usize>(&mut self) -> Result<&mut [i32; N], &'static str> {
            Ok(self
                .vec
                .get_mut(self.offset..self.offset + N)
                .ok_or("range")?
                .try_into()
                .unwrap())
        }
    }
    let mut s = S::default();
    s.vec.resize(10, 0);
    s.set_json("/offset", b"3").unwrap();
    s.set_json("/arr/1", b"5").unwrap();
    assert_eq!(s.vec[s.offset + 1], 5);
    let mut buf = [0; 10];
    let len = s.get_json("/arr/1", &mut buf[..]).unwrap();
    assert_eq!(buf[..len], b"5"[..]);
    s.set_json("/offset", b"100").unwrap();
    assert_eq!(
        s.set_json("/arr/1", b"5"),
        Err(Traversal::Access(1, "range").into())
    );
}
