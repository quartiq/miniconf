#![cfg(feature = "json")]

use miniconf::{JsonSlash, Miniconf};

#[test]
fn struct_with_string() {
    #[derive(Miniconf, Default)]
    struct Settings {
        string: String,
    }

    let mut s = Settings::default();

    let mut buf = [0u8; 256];
    let len = s.get_json("/string", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"\"\"");

    s.set_json("/string", br#""test""#).unwrap();
    assert_eq!(s.string, "test");
}
