#![cfg(feature = "json")]

use miniconf::{Miniconf, SerDe};

#[test]
fn struct_with_string() {
    #[derive(Miniconf, Default)]
    struct Settings {
        string: String,
    }

    let mut s = Settings::default();

    let mut buf = [0u8; 256];
    let len = s.get("/string", &mut buf).unwrap();
    assert_eq!(&buf[..len], b"\"\"");

    s.set("/string", br#""test""#).unwrap();
    assert_eq!(s.string, "test");
}
