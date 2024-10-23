use miniconf::{Metadata, Tree};

#[derive(Tree)]
struct S<T>(Option<Option<T>>);

fn main() {
    // does not compile as u32 does not implement Tree
    S::<u32>::traverse_all::<Metadata>();
}
