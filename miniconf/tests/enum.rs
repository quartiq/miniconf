use miniconf::{
    json, Keys, Leaf, SerDeError, StrLeaf, Tree, TreeDeserialize, TreeSchema, TreeSerialize,
};

mod common;
use common::*;
use serde::{Deserializer, Serializer};

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: Leaf<i32>,
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
    A(Leaf<i32>),
    B(Inner),
}

#[derive(Tree, Default, Debug)]
struct Settings {
    #[tree(typ="Leaf<EnumDiscriminants>", rename=tag,
        with(serialize=self.enum_serialize, deserialize=self.enum_deserialize),
        deny(ref_any="deny", mut_any="deny"))]
    _tag: (),
    enu: Enum,

    // Alternative with StrLeaf
    #[tree(rename = "tag_str")]
    enu_str: StrLeaf<Enum>,
    #[tree(rename = "enu_str", typ = "Enum", defer = "(*self.enu_str)")]
    _enu_str: (),
}

impl Settings {
    fn enum_serialize<K: Keys, S: Serializer>(
        &self,
        keys: K,
        ser: S,
    ) -> Result<S::Ok, SerDeError<S::Error>> {
        Leaf(EnumDiscriminants::from(&self.enu)).serialize_by_key(keys, ser)
    }

    fn enum_deserialize<'de, K: Keys, D: Deserializer<'de>>(
        &mut self,
        keys: K,
        de: D,
    ) -> Result<(), SerDeError<D::Error>> {
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
        Err(SerDeError::Inner(serde_json_core::de::Error::CustomError))
    );
    assert_eq!(s.enu, Enum::A(0.into()));
    set_get(&mut s, "/enu/foo", b"99");
    assert_eq!(s.enu, Enum::A(99.into()));
    assert_eq!(
        json::set(&mut s, "/enu/B/a", b"99"),
        Err(miniconf::ValueError::Absent.into())
    );
    set_get(&mut s, "/tag", b"\"B\"");
    set_get(&mut s, "/enu/B/a", b"8");
    assert_eq!(s.enu, Enum::B(Inner { a: 8.into() }));

    assert_eq!(
        paths::<3>(Settings::SCHEMA),
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
        A(Leaf<i32>, #[tree(skip)] i32),
        #[tree(skip)]
        B(S),
        C,
        D,
    }
    assert_eq!(paths::<1>(E::SCHEMA), ["/A"]);
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
    assert_eq!(paths::<1>(Option::<[Leaf<i32>; 1]>::SCHEMA), ["/0"]);
    assert_eq!(
        paths::<1>(Option::<::core::option::Option<Leaf<i32>>>::SCHEMA),
        [""]
    );
}
