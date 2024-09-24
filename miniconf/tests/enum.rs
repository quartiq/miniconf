use miniconf::{JsonCoreSlash, Path, Tree, TreeDeserialize, TreeKey, TreeSerialize};
use strum::{AsRefStr, EnumString};

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
    #[tree(typ = "&str", get = "Self::get_tag", validate = "Self::set_tag")]
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
    s.set_json("/tag", b"\"foo\"").unwrap();
    assert_eq!(
        s.set_json("/tag", b"\"bar\""),
        Err(miniconf::Traversal::Invalid(1, "invalid tag").into())
    );
    assert_eq!(s.en, Enum::A(0));
    s.set_json("/en/foo", b"99").unwrap();
    assert_eq!(s.en, Enum::A(99));
    assert_eq!(
        s.set_json("/en/B/a", b"99"),
        Err(miniconf::Traversal::Absent(2).into())
    );
    s.set_json("/tag", b"\"B\"").unwrap();
    s.set_json("/en/B/a", b"8").unwrap();
    assert_eq!(s.en, Enum::B(Inner { a: 8 }));

    assert_eq!(
        Settings::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|pn| {
                let (p, n) = pn.unwrap();
                assert!(n.is_leaf());
                p.into_inner()
            })
            .collect::<Vec<_>>(),
        vec!["/tag", "/en/foo", "/en/B/a"]
    );
}
