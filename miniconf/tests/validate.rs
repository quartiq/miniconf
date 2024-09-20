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

#[test]
fn enable_option() {
    // This may be less desirable as enable and the variant are redundant and can
    // become desynced by direct writes.
    // Also it forgets the Some value.
    #[derive(Default, Tree)]
    struct S {
        #[tree(depth = 1)]
        opt: Option<i32>,
        #[tree(validate=Self::validate)]
        enable: bool,
    }

    impl S {
        fn validate(&mut self, en: bool) -> Result<bool, &'static str> {
            if en {
                if self.opt.is_none() {
                    self.opt = Some(Default::default());
                }
            } else {
                self.opt = None;
            }
            Ok(en)
        }
    }

    let mut s = S::default();
    s.set_json("/enable", b"true").unwrap();
    s.set_json("/opt", b"1").unwrap();
    assert_eq!(s.opt, Some(1));
    s.set_json("/enable", b"false").unwrap();
    assert_eq!(s.opt, None);
    s.set_json("/opt", b"1").unwrap_err();
}

#[test]
fn locked() {
    // This is a bit nicer (could also be called `lock`, or be a `Access` enum)
    // It doesn't show up as `Absent` though.
    #[derive(Default, Tree)]
    struct S {
        #[tree(get=Self::get, get_mut=Self::get_mut)]
        val: i32,
        read: bool,
        write: bool,
    }

    impl S {
        fn get(&self) -> Result<&i32, &'static str> {
            if self.read {
                Ok(&self.val)
            } else {
                Err("not readable")
            }
        }
        fn get_mut(&mut self) -> Result<&mut i32, &'static str> {
            if self.write {
                Ok(&mut self.val)
            } else {
                Err("not writable")
            }
        }
    }

    let mut s = S::default();
    s.set_json("/write", b"true").unwrap();
    s.set_json("/val", b"1").unwrap();
    assert_eq!(s.val, 1);
    s.set_json("/write", b"false").unwrap();
    assert_eq!(s.val, 1);
    s.set_json("/val", b"1").unwrap_err();
}

#[test]
fn write_only() {
    #[derive(Default, Tree)]
    struct S {
        #[tree(typ="[i32; 0]", get=Self::get, get_mut=Self::get_mut, validate=Self::validate)]
        v: (),
    }

    impl S {
        fn get(&self) -> Result<&(), &'static str> {
            Ok(&())
        }

        fn get_mut(&mut self) -> Result<&mut (), &'static str> {
            Ok(&mut self.v)
        }

        fn validate(&mut self, val: &str) -> Result<(), &'static str> {
            assert_eq!(val, "foo");
            Ok(())
        }
    }

    let mut s = S::default();
    s.set_json("/v", b"\"foo\"").unwrap();
    let mut buf = [0u8; 10];
    let len = s.get_json("/v", &mut buf[..]).unwrap();
    assert_eq!(&buf[..len], b"null");
}
