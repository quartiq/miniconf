use miniconf::Tree;

#[derive(Tree)]
#[tree(attrs(foo))]
struct S(i32);

#[derive(Tree)]
#[tree(attrs(doc = "foo"))]
/// Docs
struct T(i32);

fn main() {}
