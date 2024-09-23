use std::cmp::max;

use miniconf::{JsonCoreSlash, Path, Tree, TreeDeserialize, TreeKey, TreeSerialize};

// #[cfg(feature = "no")]
#[test]
fn newtype_enums() {
    #[derive(TreeKey, Default)]
    struct Inner {
        a: i32,
    }

    #[derive(TreeKey, Default)]
    enum Settings {
        #[default]
        Unit,
        Tuple(i32),
        Depth(#[tree(depth = 1)] Inner),
    }

    let mut s = Settings::default();

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

    // let mut buf = [0u8; 256];
    // let len = s.get_json("/variant", &mut buf).unwrap();
    // assert_eq!(&buf[..len], b"Unit");

    // s.set_json("/variant", b"3.0").unwrap();
    // assert_eq!(s.1, 3.0);
    // s.set_json("/2", b"3.0").unwrap_err();
    // s.set_json("/foo", b"3.0").unwrap_err();
}
