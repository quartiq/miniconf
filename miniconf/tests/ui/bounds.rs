#[allow(unused_imports)]
use miniconf::TreeKey;
use miniconf::{Metadata, Tree, Leaf};

#[derive(Tree)]
struct S<T>(Option<Option<T>>);

fn main() {
    // This is fine:
    S::<[Leaf<u32>; 3]>::traverse_all::<Metadata>();
    // This does not compile as u32 does not implement TreeKey
    S::<u32>::traverse_all::<Metadata>();
}
