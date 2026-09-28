use miniconf::TreeSchema;

#[derive(TreeSchema)]
struct Settings {
    x: u8,
    #[tree(rename = "x")]
    y: u16,
}

#[derive(TreeSchema)]
enum Mode {
    A(u8),
    #[tree(rename = "A")]
    B(u16),
}

fn main() {}
