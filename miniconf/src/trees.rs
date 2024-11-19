use crate::{IntoKeys, Node, Transcode, Traversal, TreeKey};

use trees::Tree;

/// Build a [`trees::Tree`] of keys for a `TreeKey`.
pub fn tree<M: TreeKey, K: Transcode + Default>(
    state: &mut [usize],
    depth: usize,
) -> Result<Tree<(K, Node)>, Traversal> {
    let mut root = Tree::new(M::transcode(state[..depth].into_keys())?);
    if !root.data().1.is_leaf() && depth < state.len() {
        debug_assert_eq!(state[depth], 0);
        loop {
            match tree::<M, _>(state, depth + 1) {
                Ok(child) => {
                    debug_assert_eq!(child.data().1.depth(), depth + 1);
                    root.push_back(child);
                    state[depth] += 1;
                }
                Err(Traversal::NotFound(d)) => {
                    debug_assert_eq!(d, depth + 1);
                    state[depth] = 0;
                    break;
                }
                e => {
                    return e;
                }
            }
        }
    }
    Ok(root)
}
