use miniconf::{ExactSize, NodeIter, TreeSchema};

const _: ExactSize<NodeIter<(), 0>> = <[(); 1]>::SCHEMA.nodes();

fn main() {}
