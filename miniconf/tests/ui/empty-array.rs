use miniconf::TreeSchema;

fn main() {
    const _: usize = <[usize; 0] as TreeSchema>::SCHEMA.shape().max_depth;
}
