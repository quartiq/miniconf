use miniconf::{SerdeError, Tree, ValueError, json, str_leaf};

mod common;
use common::*;

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
    #[tree(rename="tag", with=str_leaf, defer=self.enu, typ="Enum")]
    _tag: (),
    enu: Enum,
}

#[test]
fn enum_switch() {
    let mut s = Settings::default();
    assert_eq!(s.enu, Enum::None);
    set_get(&mut s, "/tag", b"\"foo\"");
    assert!(matches!(
        json::set(&mut s, "/tag", b"\"bar\""),
        Err(SerdeError::Value(ValueError::Access(_)))
    ));
    assert_eq!(s.enu, Enum::A(0));
    set_get(&mut s, "/enu/foo", b"99");
    assert_eq!(s.enu, Enum::A(99));
    assert_eq!(
        json::set(&mut s, "/enu/B/a", b"99"),
        Err(ValueError::Absent.into())
    );
    set_get(&mut s, "/tag", b"\"B\"");
    set_get(&mut s, "/enu/B/a", b"8");
    assert_eq!(s.enu, Enum::B(Inner { a: 8 }));

    assert_eq!(paths::<Settings, 3>(), ["/tag", "/enu/foo", "/enu/B/a",]);
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
