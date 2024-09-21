use miniconf::{JsonCoreSlash, Path, Tree, TreeDeserialize, TreeKey, TreeSerialize};

#[cfg(feature = "no")]
#[test]
fn tuple_struct() {
    #[derive(Tree, Default)]
    enum Settings {
        #[default]
        Unit,
        Struct {
            a: i32,
            b: i32,
        },
        Tuple(i32, i32),
    }

    let mut s = Settings::default();

    let mut buf = [0u8; 256];
    let len = s.get_json("/variant", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"Unit");

    s.set_json("/variant", b"3.0").unwrap();
    assert_eq!(s.1, 3.0);
    s.set_json("/2", b"3.0").unwrap_err();
    s.set_json("/foo", b"3.0").unwrap_err();

    assert_eq!(
        Settings::nodes::<Path<String, '/'>>()
            .exact_size()
            .map(|p| p.unwrap().0.into_inner())
            .collect::<Vec<_>>(),
        vec!["/0", "/1"]
    );
}
