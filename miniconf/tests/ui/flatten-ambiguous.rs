use miniconf::Tree;

#[derive(Tree)]
#[tree(flatten)]
pub struct S {
    a: i32,
    b: i32,
}

#[derive(Tree)]
#[tree(flatten)]
pub enum E {
    A(i32),
    B(i32),
}

fn main() {}
