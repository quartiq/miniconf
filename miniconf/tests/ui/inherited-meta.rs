use miniconf::Tree;

#[derive(Tree)]
#[tree(meta(foo))]
struct S(i32);

#[derive(Tree)]
#[tree(meta(doc = "foo"))]
/// Docs
struct T(i32);

fn main() {}
