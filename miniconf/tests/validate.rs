use miniconf::{json, Leaf, Traversal, Tree};

#[derive(Tree, Default)]
struct Inner {
    a: Leaf<f32>,
}

#[derive(Tree, Default)]
struct Settings {
    #[tree(validate=self.validate_v)]
    v: Leaf<f32>,
    #[tree(validate=self.validate_i)]
    i: Inner,
}

impl Settings {
    fn validate_v(&mut self, depth: usize) -> Result<usize, &'static str> {
        if *self.v >= 0.0 {
            Ok(depth)
        } else {
            Err("")
        }
    }

    fn validate_i(&mut self, depth: usize) -> Result<usize, &'static str> {
        if *self.i.a >= 0.0 {
            Ok(depth)
        } else {
            Err("")
        }
    }
}

#[test]
fn validate() {
    let mut s = Settings::default();
    json::set(&mut s, "/v", b"1.0").unwrap();
    assert_eq!(*s.v, 1.0);
    assert_eq!(
        json::set(&mut s, "/v", b"-1.0"),
        Err(Traversal::Invalid(1, "").into())
    );
    // TODO
    // assert_eq!(*s.v, 1.0); // remains unchanged
    json::set(&mut s, "/i/a", b"1.0").unwrap();
    assert_eq!(*s.i.a, 1.0);
    assert_eq!(
        json::set(&mut s, "/i/a", b"-1.0"),
        Err(Traversal::Invalid(1, "").into())
    );
    assert_eq!(*s.i.a, -1.0); // has changed as internal validation was done after leaf setting
    assert_eq!(json::set(&mut s, "/i/a", b"1.0"), Ok(3));
}

#[test]
fn other_type() {
    // Demonstrate and test how a variable length `Vec` can be accessed
    // through a variable offset, fixed length array.
    #[derive(Default, Tree)]
    struct S {
        #[tree(typ="[Leaf<i32>; 4]", get=self.get::<4>(), get_mut=self.get_mut::<4>(), rename=arr)]
        vec: Vec<Leaf<i32>>,
        offset: Leaf<usize>,
    }
    impl S {
        fn get<const N: usize>(&self) -> Result<&[Leaf<i32>; N], &'static str> {
            Ok(self
                .vec
                .get(*self.offset..*self.offset + N)
                .ok_or("range")?
                .try_into()
                .unwrap())
        }
        fn get_mut<const N: usize>(&mut self) -> Result<&mut [Leaf<i32>; N], &'static str> {
            Ok(self
                .vec
                .get_mut(*self.offset..*self.offset + N)
                .ok_or("range")?
                .try_into()
                .unwrap())
        }
    }
    let mut s = S::default();
    s.vec.resize(10, 0.into());
    json::set(&mut s, "/offset", b"3").unwrap();
    json::set(&mut s, "/arr/1", b"5").unwrap();
    assert_eq!(s.vec[*s.offset + 1], 5.into());
    let mut buf = [0; 10];
    let len = json::get(&s, "/arr/1", &mut buf[..]).unwrap();
    assert_eq!(buf[..len], b"5"[..]);
    json::set(&mut s, "/offset", b"100").unwrap();
    assert_eq!(
        json::set(&mut s, "/arr/1", b"5"),
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
        opt: Option<Leaf<i32>>,
        #[tree(validate=self.validate)]
        enable: Leaf<bool>,
    }

    impl S {
        fn validate(&mut self, depth: usize) -> Result<usize, &'static str> {
            if *self.enable {
                if self.opt.is_none() {
                    self.opt = Some(Default::default());
                }
            } else {
                self.opt = None;
            }
            Ok(depth)
        }
    }

    let mut s = S::default();
    json::set(&mut s, "/enable", b"true").unwrap();
    json::set(&mut s, "/opt", b"1").unwrap();
    assert_eq!(s.opt, Some(1.into()));
    json::set(&mut s, "/enable", b"false").unwrap();
    assert_eq!(s.opt, None);
    json::set(&mut s, "/opt", b"1").unwrap_err();
}

#[test]
fn locked() {
    // This is a bit nicer (could also be called `lock`, or be a `Access` enum)
    // It doesn't show up as `Absent` though.
    #[derive(Default, Tree)]
    struct S {
        #[tree(get=self.get(), get_mut=self.get_mut())]
        val: Leaf<i32>,
        read: Leaf<bool>,
        write: Leaf<bool>,
    }

    impl S {
        fn get(&self) -> Result<&Leaf<i32>, &'static str> {
            if *self.read {
                Ok(&self.val)
            } else {
                Err("not readable")
            }
        }
        fn get_mut(&mut self) -> Result<&mut Leaf<i32>, &'static str> {
            if *self.write {
                Ok(&mut self.val)
            } else {
                Err("not writable")
            }
        }
    }

    let mut s = S::default();
    json::set(&mut s, "/write", b"true").unwrap();
    json::set(&mut s, "/val", b"1").unwrap();
    assert_eq!(*s.val, 1);
    json::set(&mut s, "/write", b"false").unwrap();
    assert_eq!(*s.val, 1);
    json::set(&mut s, "/val", b"1").unwrap_err();
}
