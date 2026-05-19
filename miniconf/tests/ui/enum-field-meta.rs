use miniconf::Tree;

#[derive(Tree)]
pub enum E {
    A(#[tree(meta(doc = "bad"))] i32),
}

fn main() {}
