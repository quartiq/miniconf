use miniconf::{JsonCoreSlash, Path, Tree, TreeDeserialize, TreeKey, TreeSerialize};
use strum::{AsRefStr, EnumString};

#[test]
fn newtype_enums() {
    #[derive(Tree, Default)]
    struct Inner {
        a: i32,
    }

    #[derive(Tree, Default)]
    enum Settings {
        #[default]
        Unit,
        Tuple(i32),
        Defer(#[tree(depth = 1)] Inner),
    }

    assert_eq!(
        <Settings as TreeKey<2>>::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        vec!["/Tuple", "/Defer/a"]
    );

    let mut m = miniconf::Metadata::default();
    m.max_length = 6;
    m.max_depth = 2;
    m.count = 2;
    assert_eq!(Settings::metadata(), m);

    let mut s = Settings::Defer(Inner::default());
    s.set_json("/Defer/a", b"3").unwrap();
    s.set_json("/Tuple", b"9").unwrap_err();
    let mut buf = [0u8; 256];
    s.get_json("/Tuple", &mut buf).unwrap_err();
    let len = s.get_json("/Defer/a", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"3");

    let mut s = Settings::Tuple(0);
    s.set_json("/Tuple", b"9").unwrap();
    s.set_json("/Defer/a", b"9").unwrap_err();
    let len = s.get_json("/Tuple", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"9");
    s.get_json("/Defer/a", &mut buf).unwrap_err();
}

#[test]
fn enum_switch() {
    #[derive(Tree, Default)]
    struct Inner {
        a: i32,
    }

    #[derive(Tree, Default, EnumString, AsRefStr)]
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
        payload: Enum,
    }

    impl Settings {
        fn get_tag(&self) -> Result<&str, &'static str> {
            Ok(self.payload.as_ref())
        }

        fn set_tag(&mut self, tag: &str) -> Result<(), &'static str> {
            self.payload = Enum::try_from(tag).or(Err("invalid tag"))?;
            Ok(())
        }
    }

    let mut s = Settings::default();
    assert!(matches!(s.payload, Enum::None));
    s.set_json("/tag", b"\"foo\"").unwrap();
    assert_eq!(
        s.set_json("/tag", b"\"bar\""),
        Err(miniconf::Traversal::Invalid(1, "invalid tag").into())
    );
    assert!(matches!(s.payload, Enum::A(0)));
    s.set_json("/payload/foo", b"99").unwrap();
    assert!(matches!(s.payload, Enum::A(99)));
    assert_eq!(
        s.set_json("/payload/B/a", b"99"),
        Err(miniconf::Traversal::Absent(2).into())
    );
    s.set_json("/tag", b"\"B\"").unwrap();
    s.set_json("/payload/B/a", b"8").unwrap();
    assert!(matches!(s.payload, Enum::B(Inner { a: 8 })));

    assert_eq!(
        Settings::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|pn| {
                let (p, n) = pn.unwrap();
                assert!(n.is_leaf());
                p.into_inner()
            })
            .collect::<Vec<_>>(),
        vec!["/tag", "/payload/foo", "/payload/B/a"]
    );
}
