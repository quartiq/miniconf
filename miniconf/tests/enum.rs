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
    #[derive(Tree, Default, EnumString, Clone, AsRefStr)]
    enum Settings {
        #[default]
        None,
        #[tree(rename = "a")]
        #[strum(serialize = "a")]
        A(i32),
        #[strum(serialize = "b")]
        #[tree(rename = "b")]
        B(f32),
    }

    #[derive(TreeKey, TreeSerialize, TreeDeserialize, Default)]
    struct Outer {
        #[tree(typ = "&str", get = "Self::get_tag", validate = "Self::set_tag")]
        tag: (),
        #[tree(depth = 1)]
        payload: Settings,
    }

    impl Outer {
        fn get_tag(&self) -> Result<&str, &'static str> {
            Ok(self.payload.as_ref())
        }

        fn set_tag(&mut self, tag: &str) -> Result<(), &'static str> {
            self.payload = Settings::try_from(tag).map_err(|_| "invalid tag")?;
            Ok(())
        }
    }

    let mut s = Outer::default();
    assert!(matches!(s.payload, Settings::None));
    s.set_json("/tag", b"\"a\"").unwrap();
    assert!(matches!(s.payload, Settings::A(0)));
    s.set_json("/payload/a", b"99").unwrap();
    assert!(matches!(s.payload, Settings::A(99)));
    assert_eq!(
        s.set_json("/payload/b", b"99"),
        Err(miniconf::Traversal::Absent(2).into())
    );
    assert_eq!(
        Outer::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(Result::unwrap)
            .map(|(p, n)| {
                assert!(n.is_leaf());
                p.into_inner()
            })
            .collect::<Vec<_>>(),
        vec!["/tag", "/payload/a", "/payload/b"]
    );
}
