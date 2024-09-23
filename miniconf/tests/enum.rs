use miniconf::{JsonCoreSlash, Path, Tree, TreeDeserialize, TreeKey, TreeSerialize};

// #[cfg(feature = "no")]
#[test]
fn newtype_enums() {
    #[derive(Tree, Default)]
    struct Inner {
        a: i32,
    }

    #[derive(TreeKey, TreeSerialize, TreeDeserialize, Default)]
    enum Settings {
        #[default]
        Unit,
        Tuple(i32),
        Depth(#[tree(depth = 1)] Inner),
    }

    assert_eq!(
        <Settings as TreeKey<2>>::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        vec!["/Tuple", "/Depth/a"]
    );

    let mut m = miniconf::Metadata::default();
    m.max_length = 6;
    m.max_depth = 2;
    m.count = 2;
    assert_eq!(Settings::metadata(), m);

    let mut s = Settings::Depth(Inner::default());
    s.set_json("/Depth/a", b"3").unwrap();
    s.set_json("/Tuple", b"9").unwrap_err();
    let mut buf = [0u8; 256];
    s.get_json("/Tuple", &mut buf).unwrap_err();
    let len = s.get_json("/Depth/a", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"3");

    let mut s = Settings::Tuple(0);
    s.set_json("/Tuple", b"9").unwrap();
    s.set_json("/Depth/a", b"9").unwrap_err();
    let len = s.get_json("/Tuple", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"9");
    s.get_json("/Depth/a", &mut buf).unwrap_err();
}
