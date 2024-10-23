use miniconf::{json, Leaf, Tree, TreeDeserialize, TreeKey, TreeSerialize};
use strum::{AsRefStr, EnumString};

mod common;
use common::*;

#[derive(Tree, Default, PartialEq, Debug)]
struct Inner {
    a: Leaf<i32>,
}

#[derive(Tree, Default, EnumString, AsRefStr, PartialEq, Debug)]
enum Enum {
    #[default]
    None,
    #[strum(serialize = "foo")]
    #[tree(rename = "foo")]
    A(Leaf<i32>),
    B(Inner),
}

// #[derive(TreeKey, TreeSerialize, TreeDeserialize, Default)]
// struct Settings {
//     #[tree(typ = "Leaf<&'static str>", get = Self::get_tag, get_mut = Self::get_mut, validate = Self::set_tag)]
//     tag: (),
//     en: Enum,
// }

// impl Settings {
//     fn get_tag(&self) -> Result<&Leaf<&str>, &'static str> {
//         Ok(&Leaf(self.en.as_ref()))
//     }
//     fn get_mut(&mut self) -> Result<&mut Leaf<&str>, &'static str> {

//     }

//     fn set_tag(&mut self, depth: usize) -> Result<usize, &'static str> {
//         self.en = Enum::try_from(tag).or(Err("invalid tag"))?;
//         Ok(())
//     }
// }

// #[test]
// fn enum_switch() {
//     let mut s = Settings::default();
//     assert_eq!(s.en, Enum::None);
//     set_get(&mut s, "/tag", b"\"foo\"");
//     assert_eq!(
//         json::set(&mut s, "/tag", b"\"bar\""),
//         Err(miniconf::Traversal::Invalid(1, "invalid tag").into())
//     );
//     assert_eq!(s.en, Enum::A(0.into()));
//     set_get(&mut s, "/en/foo", b"99");
//     assert_eq!(s.en, Enum::A(99.into()));
//     assert_eq!(
//         json::set(&mut s, "/en/B/a", b"99"),
//         Err(miniconf::Traversal::Absent(2).into())
//     );
//     set_get(&mut s, "/tag", b"\"B\"");
//     set_get(&mut s, "/en/B/a", b"8");
//     assert_eq!(s.en, Enum::B(Inner { a: 8.into() }));

//     assert_eq!(paths::<Settings>(), ["/tag", "/en/foo", "/en/B/a"]);
// }

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
    assert_eq!(paths::<E>(), ["/A"]);
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
        Some(T),
    }
    assert_eq!(paths::<Option<[Leaf<i32>; 1]>>(), ["/0"]);
    assert_eq!(paths::<Option<::core::option::Option<Leaf<i32>>>>(), [""]);
}
