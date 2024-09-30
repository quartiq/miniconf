use miniconf::Tree;

#[derive(Tree)]
pub enum E1 {
    A(i32, i32),
}

#[derive(Tree)]
pub enum E2 {
    A { a: i32, b: i32 },
}

fn main() {}
