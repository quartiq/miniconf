use miniconf::Tree;

#[derive(Tree)]
pub struct S(#[tree(skip)] i32, i32);

#[derive(Tree)]
pub enum E {
    A(#[tree(skip)] i32, i32),
}

fn main() {}
