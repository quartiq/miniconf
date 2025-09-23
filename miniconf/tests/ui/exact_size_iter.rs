use miniconf::{ExactSize, NodeIter};

const _: ExactSize<NodeIter<(), 0>> = NodeIter::exact_size::<[(); 1]>();

fn main() {}
