use crate::{IntoKeys, KeyLookup, Node, Transcode, Traversal, TreeKey, Walk};

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

struct TreeWalk(Tree<Option<KeyLookup>>);

impl Walk for TreeWalk {
    type Error = core::convert::Infallible;

    fn internal() -> Self {
        Self(Tree::new(None))
    }
    fn leaf() -> Self {
        Self(Tree::new(None))
    }
    fn merge(
        mut self,
        walk: &Self,
        index: Option<usize>,
        lookup: &KeyLookup,
    ) -> Result<Self, Self::Error> {
        if let Some(l) = self.0.root().data() {
            debug_assert_eq!(l, lookup);
        }
        self.0
            .root_mut()
            .data_mut()
            .get_or_insert_with(|| lookup.clone());
        if let Some(index) = index {
            debug_assert_eq!(index, self.0.root().degree());
        }
        self.0.push_back(walk.0.clone());
        Ok(self)
    }
}

/// Build a Tree of KeyLookup
pub fn tree_all<M: TreeKey>() -> Tree<Option<KeyLookup>> {
    M::traverse_all::<TreeWalk>().unwrap().0
}
