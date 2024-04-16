use miniconf::{Error, IntoKeys, Packed, Tree, TreeKey};

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

    assert_eq!(
        Settings::path(Packed::new(0b10).unwrap(), &mut p, "/"),
        Ok(1)
    );
    assert_eq!(p, "/a");
    p.clear();

    assert_eq!(
        Settings::path(Packed::new(0b111).unwrap(), &mut p, "/"),
        Ok(2)
    );
    assert_eq!(p, "/b/1");

    assert_eq!(Settings::packed(["a"]), Ok(Packed::new(0b10).unwrap()));
    assert_eq!(
        Settings::packed(["b", "0"]),
        Ok(Packed::new(0b110).unwrap())
    );
    assert_eq!(
        Settings::packed(["b", "1"]),
        Ok(Packed::new(0b111).unwrap())
    );
}
