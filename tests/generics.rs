use miniconf::Miniconf;
use serde::Serialize;

#[test]
fn generic_struct() {
    #[derive(Miniconf, Serialize, Default)]
    struct Settings<T: Miniconf> {
        data: T,
    }

    let mut settings = Settings::<f32>::default();
    settings
        .string_set("data".split('/').peekable(), b"3.0")
        .unwrap();
}
