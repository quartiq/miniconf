use miniconf::Miniconf;

#[derive(Miniconf, Default)]
struct Inner {
    inner: f32,
}

#[derive(Miniconf, Default)]
struct Settings {
    a: f32,
    b: i32,
    #[miniconf(defer)]
    c: Inner,
}

#[test]
fn meta() {
    let meta = Settings::metadata(1);
    assert_eq!(meta.max_depth, 2);
    assert_eq!(meta.max_length, "/c/inner".len());
    assert_eq!(meta.count, 3);
}

#[test]
fn next_path() {
    let mut s = String::new();
    Settings::next_path(&[1, 0, 0], 0, &mut s, '/').unwrap();
    assert_eq!(s, "/b");
    s.clear();
    Settings::next_path(&[2, 0, 0], 0, &mut s, '/').unwrap();
    assert_eq!(s, "/c/inner");
}
