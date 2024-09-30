use miniconf::{JsonCoreSlash, Tree, TreeDeserialize, TreeKey, TreeSerialize};
use strum::{AsRefStr, EnumString};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: i32,
}

#[derive(Tree, Default, EnumString, AsRefStr, PartialEq, Debug)]
enum Enum {
    #[default]
    None,
    #[strum(serialize = "foo")]
    #[tree(rename = "foo")]
    A(i32),
    B(#[tree(depth = 1)] Inner),
}

#[derive(TreeKey, TreeSerialize, TreeDeserialize, Default)]
struct Settings {
    #[tree(typ = "&str", get = Self::get_tag, validate = Self::set_tag)]
    tag: (),
    #[tree(depth = 2)]
    en: Enum,
}

impl Settings {
    fn get_tag(&self) -> Result<&str, &'static str> {
        Ok(self.en.as_ref())
    }

    fn set_tag(&mut self, tag: &str) -> Result<(), &'static str> {
        self.en = Enum::try_from(tag).or(Err("invalid tag"))?;
        Ok(())
    }
}

#[test]
fn enum_switch() {
    let mut s = Settings::default();
    assert_eq!(s.en, Enum::None);
    set_get(&mut s, "/tag", b"\"foo\"");
    assert_eq!(
        s.set_json("/tag", b"\"bar\""),
        Err(miniconf::Traversal::Invalid(1, "invalid tag").into())
    );
    assert_eq!(s.en, Enum::A(0));
    set_get(&mut s, "/en/foo", b"99");
    assert_eq!(s.en, Enum::A(99));
    assert_eq!(
        s.set_json("/en/B/a", b"99"),
        Err(miniconf::Traversal::Absent(2).into())
    );
    set_get(&mut s, "/tag", b"\"B\"");
    set_get(&mut s, "/en/B/a", b"8");
    assert_eq!(s.en, Enum::B(Inner { a: 8 }));

    assert_eq!(paths::<Settings, 3>(), ["/tag", "/en/foo", "/en/B/a"]);
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
        // #192
        Some(#[tree(depth = 1)] T),
    }
    assert_eq!(paths::<Option<[i32; 1]>, 1>(), ["/0"]);
    assert_eq!(paths::<Option<::core::option::Option<i32>>, 1>(), [""]);
}
