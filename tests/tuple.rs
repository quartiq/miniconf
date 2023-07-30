#![cfg(feature = "json-core")]

use miniconf::{JsonCoreSlash, Miniconf};

#[test]
fn tuple_struct() {
    #[derive(Miniconf, Default)]
    struct Settings(i32, f32);

    let mut s = Settings::default();

    let mut buf = [0u8; 256];
    let len = s.get_json("/0", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"0");

    s.set_json("/1", b"3.0").unwrap();
    assert_eq!(s.1, 3.0);
}
