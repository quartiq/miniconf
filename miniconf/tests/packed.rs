use miniconf::{Error, Packed, Tree, TreeKey};

#[derive(Tree, Default)]
struct Settings {
    a: f32,
    #[tree(depth = 1)]
    b: [f32; 2],
}

#[test]
fn packed() {
    let mut p = String::new();

    assert_eq!(
        Settings::path(Packed::default(), &mut p, "/"),
        Err(Error::TooShort(0))
    );
    p.clear();

    for q in Settings::iter_paths::<String>("/") {
        let q = q.unwrap();
        let (a, _d) = Settings::packed(q.split("/").skip(1)).unwrap();
        Settings::path(a, &mut p, "/").unwrap();
        assert_eq!(p, q);
        p.clear();
    }

    assert_eq!(
        Settings::path(Packed::new(0b01 << 29).unwrap(), &mut p, "/"),
        Ok(1)
    );
    assert_eq!(p, "/a");
    p.clear();
}
