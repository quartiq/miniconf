use miniconf::{json, Tree, ValueError};

#[derive(Tree, Default)]
struct Check {
    #[tree(with(all=check))]
    v: f32,
}

mod check {
    use miniconf::{Deserializer, Keys, SerdeError, TreeDeserialize, ValueError};

    pub use miniconf::leaf::{
        mut_any_by_key, probe_by_key, ref_any_by_key, serialize_by_key, SCHEMA,
    };

    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        value: &mut f32,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        let mut old = *value;
        old.deserialize_by_key(keys, de)?;
        if old < 0.0 {
            Err(ValueError::Access("").into())
        } else {
            *value = old;
            Ok(())
        }
    }
}

#[test]
fn validate() {
    let mut s = Check::default();
    json::set(&mut s, "/v", b"1.0").unwrap();
    assert_eq!(s.v, 1.0);
    assert_eq!(
        json::set(&mut s, "/v", b"-1.0"),
        Err(ValueError::Access("").into())
    );
    assert_eq!(s.v, 1.0); // remains unchanged
}

// Demonstrate and test how a variable length `Vec` can be accessed
// through a variable offset, fixed length array.
#[derive(Default, Tree)]
struct Page {
    #[tree(typ="[i32; 4]", rename=arr, defer=*self, with(all=page4))]
    vec: Vec<i32>,
    offset: usize,
}

mod page4 {
    use super::Page;

    use miniconf::{
        Deserializer, Keys, Schema, SerdeError, Serializer, TreeDeserialize, TreeSchema,
        TreeSerialize, ValueError,
    };

    const LENGTH: usize = 4;

    pub use miniconf::deny::{mut_any_by_key, probe_by_key, ref_any_by_key};

    pub const SCHEMA: &Schema = <[i32; LENGTH] as TreeSchema>::SCHEMA;

    pub fn serialize_by_key<S: Serializer>(
        value: &Page,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        let arr: &[i32; LENGTH] = value
            .vec
            .get(value.offset..value.offset + LENGTH)
            .ok_or(ValueError::Access("range"))?
            .try_into()
            .unwrap();
        arr.serialize_by_key(keys, ser)
    }

    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        value: &mut Page,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        let arr: &mut [i32; LENGTH] = value
            .vec
            .get_mut(value.offset..value.offset + LENGTH)
            .ok_or(ValueError::Access("range"))?
            .try_into()
            .unwrap();
        arr.deserialize_by_key(keys, de)
    }
}

#[test]
fn paging() {
    let mut s = Page::default();
    s.vec.resize(10, 0);
    json::set(&mut s, "/offset", b"3").unwrap();
    json::set(&mut s, "/arr/1", b"5").unwrap();
    assert_eq!(s.vec[s.offset + 1], 5);
    let mut buf = [0; 10];
    let len = json::get(&s, "/arr/1", &mut buf[..]).unwrap();
    assert_eq!(buf[..len], b"5"[..]);
    json::set(&mut s, "/offset", b"100").unwrap();
    assert_eq!(
        json::set(&mut s, "/arr/1", b"5"),
        Err(ValueError::Access("range").into())
    );
}

#[derive(Default, Tree)]
struct Lock {
    #[tree(with(all=lock), defer=*self)]
    val: i32,
    read: bool,
    write: bool,
}

mod lock {
    use super::Lock;
    use miniconf::{
        Deserializer, Keys, Schema, SerdeError, Serializer, TreeDeserialize, TreeSerialize,
        ValueError,
    };

    pub const SCHEMA: &Schema = miniconf::leaf::SCHEMA;
    pub use miniconf::deny::{mut_any_by_key, probe_by_key, ref_any_by_key};

    pub fn serialize_by_key<S: Serializer>(
        value: &Lock,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        if !value.read {
            return Err(ValueError::Access("not readable").into());
        }
        value.val.serialize_by_key(keys, ser)
    }

    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        value: &mut Lock,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        if !value.write {
            return Err(ValueError::Access("not writable").into());
        }
        value.val.deserialize_by_key(keys, de)
    }
}

#[test]
fn locked() {
    let mut s = Lock::default();
    json::set(&mut s, "/write", b"true").unwrap();
    json::set(&mut s, "/val", b"1").unwrap();
    assert_eq!(s.val, 1);
    json::set(&mut s, "/write", b"false").unwrap();
    assert_eq!(s.val, 1);
    json::set(&mut s, "/val", b"1").unwrap_err();
}
