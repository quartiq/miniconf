use miniconf::{json, Error, Keys, Leaf, Traversal, Tree, TreeDeserialize, TreeKey, TreeSerialize};
use serde::{Deserializer, Serializer};

#[derive(Tree, Default)]
struct Settings {
    #[tree(with(deserialize=self.deserialize_v))]
    v: Leaf<f32>,
}

impl Settings {
    fn deserialize_v<'de, K: Keys, D: Deserializer<'de>>(
        &mut self,
        keys: K,
        de: D,
    ) -> Result<(), Error<D::Error>> {
        let old = *self.v;
        self.v.deserialize_by_key(keys, de)?;
        if *self.v >= 0.0 {
            Ok(())
        } else {
            *self.v = old;
            Err(Traversal::Invalid(0, "").into())
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
    assert_eq!(*s.v, 1.0); // remains unchanged
}

#[test]
fn paging() {
    // Demonstrate and test how a variable length `Vec` can be accessed
    // through a variable offset, fixed length array.
    #[derive(Default, TreeKey, TreeDeserialize, TreeSerialize)]
    struct S {
        #[tree(typ="[Leaf<i32>; 4]", rename=arr,
            with(serialize=self.serialize_vec, deserialize=self.deserialize_vec))]
        vec: Vec<Leaf<i32>>,
        offset: Leaf<usize>,
    }

    impl S {
        fn serialize_vec<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, Error<S::Error>> {
            let arr: &[Leaf<i32>; 4] = self
                .vec
                .get(*self.offset..*self.offset + 4)
                .ok_or(Traversal::Access(0, "range"))?
                .try_into()
                .unwrap();
            arr.serialize_by_key(keys, ser)
        }

        fn deserialize_vec<'de, K: Keys, D: Deserializer<'de>>(
            &mut self,
            keys: K,
            de: D,
        ) -> Result<(), Error<D::Error>> {
            let arr: &mut [Leaf<i32>; 4] = self
                .vec
                .get_mut(*self.offset..*self.offset + 4)
                .ok_or(Traversal::Access(0, "range"))?
                .try_into()
                .unwrap();
            arr.deserialize_by_key(keys, de)
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
fn locked() {
    #[derive(Default, TreeKey, TreeSerialize, TreeDeserialize)]
    struct S {
        #[tree(with(serialize=self.get, deserialize=self.set))]
        val: Leaf<i32>,
        read: Leaf<bool>,
        write: Leaf<bool>,
    }

    impl S {
        fn get<K: Keys, S: Serializer>(&self, keys: K, ser: S) -> Result<S::Ok, Error<S::Error>> {
            if !*self.read {
                return Err(Traversal::Access(0, "not readable").into());
            }
            self.val.serialize_by_key(keys, ser)
        }
        fn set<'de, K: Keys, D: Deserializer<'de>>(
            &mut self,
            keys: K,
            de: D,
        ) -> Result<(), Error<D::Error>> {
            if !*self.write {
                return Err(Traversal::Access(0, "not writable").into());
            }
            self.val.deserialize_by_key(keys, de)
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
