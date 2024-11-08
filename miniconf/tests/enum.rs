use miniconf::{json, Leaf, StrLeaf, Tree};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: Leaf<i32>,
}

#[derive(Tree, Default, PartialEq, Debug, strum::EnumString, strum::AsRefStr)]
enum Enum {
    #[default]
    None,
    #[strum(serialize = "foo")]
    #[tree(rename = "foo")]
    A(Leaf<i32>),
    B(Inner),
}

#[derive(Tree, Default, Debug)]
struct Settings {
    #[tree(rename = "tag")]
    enu: StrLeaf<Enum>,
    #[tree(rename = "enu", typ = "Enum", defer = *self.enu)]
    _enu: (),
}

#[test]
fn enum_switch() {
    let mut s = Settings::default();
    assert_eq!(*s.enu, Enum::None);
    set_get(&mut s, "/tag", b"\"foo\"");
    assert_eq!(
        json::set(&mut s, "/tag", b"\"bar\""),
        Err(miniconf::Traversal::Invalid(1, "Could not convert").into())
    );
    assert_eq!(*s.enu, Enum::A(0.into()));
    set_get(&mut s, "/enu/foo", b"99");
    assert_eq!(*s.enu, Enum::A(99.into()));
    assert_eq!(
        json::set(&mut s, "/enu/B/a", b"99"),
        Err(miniconf::Traversal::Absent(2).into())
    );
    set_get(&mut s, "/tag", b"\"B\"");
    set_get(&mut s, "/enu/B/a", b"8");
    assert_eq!(*s.enu, Enum::B(Inner { a: 8.into() }));

    assert_eq!(paths::<Settings, 3>(), ["/tag", "/enu/foo", "/enu/B/a"]);
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
    assert_eq!(paths::<Option<[Leaf<i32>; 1]>, 1>(), ["/0"]);
    assert_eq!(
        paths::<Option<::core::option::Option<Leaf<i32>>>, 1>(),
        [""]
    );
}
