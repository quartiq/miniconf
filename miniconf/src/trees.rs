//! Tools to convert from TreeKey nodes to `trees::Tree`

use crate::{IntoKeys, KeyLookup, Node, Transcode, Traversal, TreeKey, Walk};

use trees::Tree;

/// Build a [`trees::Tree`] of keys for a `TreeKey`.
pub fn nodes<M: TreeKey, K: Transcode + Default>(
    state: &mut [usize],
    depth: usize,
) -> Result<Tree<(K, Node)>, Traversal> {
    let mut root = Tree::new(M::transcode(state[..depth].into_keys())?);
    if !root.data().1.is_leaf() && depth < state.len() {
        debug_assert_eq!(state[depth], 0);
        loop {
            match nodes::<M, _>(state, depth + 1) {
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

    fn leaf() -> Self {
        Self(Tree::new(None))
    }
    fn internal(children: &[&Self], lookup: &KeyLookup) -> Result<Self, Self::Error> {
        let mut root = Tree::new(Some(lookup.clone()));
        for child in children.iter() {
            root.push_back(child.0.clone());
        }
        Ok(Self(root))
    }
}

/// Build a Tree of KeyLookup
pub fn all<M: TreeKey>() -> Tree<Option<KeyLookup>> {
    M::traverse_all::<TreeWalk>().unwrap().0
}
