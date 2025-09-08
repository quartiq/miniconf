use miniconf::{json, str_leaf, Keys, Leaf, SerdeError, Tree, TreeDeserialize, TreeSerialize};

mod common;
use common::*;
use serde::{Deserializer, Serializer};

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: i32,
}

#[derive(
    Tree,
    Default,
    PartialEq,
    Debug,
    strum::EnumString,
    strum::AsRefStr,
    strum::FromRepr,
    strum::EnumDiscriminants,
)]
#[strum_discriminants(derive(Default, serde::Serialize, serde::Deserialize))]
enum Enum {
    #[default]
    #[strum_discriminants(default)]
    None,
    #[strum(serialize = "foo")]
    #[strum_discriminants(serde(rename = "foo"))]
    #[tree(rename = "foo")]
    A(i32),
    B(Inner),
}

#[derive(Tree, Default, Debug)]
struct Settings {
    // note the order allows sequential deseserialization
    #[tree(typ="Leaf<EnumDiscriminants>", rename=tag,
        with(serialize=self.enum_serialize, deserialize=self.enum_deserialize),
        deny(ref_any="deny", mut_any="deny"))]
    _tag: (),
    enu: Enum,

    // Alternative with str_leaf
    // note the order allows sequential deseserialization
    #[tree(rename="tag_str", with(all=str_leaf), defer=self.enu_str, typ="Enum")]
    _tag_str: (),
    enu_str: Enum,
}

impl Settings {
    fn enum_serialize<K: Keys, S: Serializer>(
        &self,
        keys: K,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        Leaf(EnumDiscriminants::from(&self.enu)).serialize_by_key(keys, ser)
    }

    fn enum_deserialize<'de, K: Keys, D: Deserializer<'de>>(
        &mut self,
        keys: K,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        let mut v = Leaf(EnumDiscriminants::from(&self.enu));
        v.deserialize_by_key(keys, de)?;
        self.enu = Enum::from_repr(*v as _).unwrap();
        Ok(())
    }
}

#[test]
fn enum_switch() {
    let mut s = Settings::default();
    assert_eq!(s.enu, Enum::None);
    set_get(&mut s, "/tag", b"\"foo\"");
    assert_eq!(
        json::set(&mut s, "/tag", b"\"bar\""),
        Err(SerdeError::Inner(serde_json_core::de::Error::CustomError))
    );
    assert_eq!(s.enu, Enum::A(0));
    set_get(&mut s, "/enu/foo", b"99");
    assert_eq!(s.enu, Enum::A(99));
    assert_eq!(
        json::set(&mut s, "/enu/B/a", b"99"),
        Err(miniconf::ValueError::Absent.into())
    );
    set_get(&mut s, "/tag", b"\"B\"");
    set_get(&mut s, "/enu/B/a", b"8");
    assert_eq!(s.enu, Enum::B(Inner { a: 8 }));

    assert_eq!(
        paths::<Settings, 3>(),
        [
            "/tag",
            "/enu/foo",
            "/enu/B/a",
            "/tag_str",
            "/enu_str/foo",
            "/enu_str/B/a",
        ]
    );
}

#[test]
fn enum_skip() {
    struct S;

    #[allow(dead_code)]
    #[derive(Tree)]
    enum E {
        A(i32, #[tree(skip)] i32),
        #[tree(skip)]
        B(S),
        C,
        D,
    }
    assert_eq!(paths::<E, 1>(), ["/A"]);
}

#[test]
fn option() {
    // Also tests macro hygiene a bit
    #[allow(dead_code)]
    #[derive(Tree, Copy, Clone, PartialEq, Default, Debug)]
    #[tree(flatten)]
    enum Option<T> {
        #[default]
        None,
        Some(T),
    }
    assert_eq!(paths::<Option::<[i32; 1]>, 1>(), ["/0"]);
    assert_eq!(paths::<Option::<::core::option::Option<i32>>, 1>(), [""]);
}
