use crate::{Indices, IntoKeys, KeyLookup, Keys, KeysIter, Node, Transcode, Traversal, TreeKey};
use core::{
    iter::{Copied, FusedIterator},
    marker::PhantomData,
    slice::Iter,
};

/// Counting wrapper for iterators with known size
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactSize<T> {
    iter: T,
    count: usize,
}

impl<T> ExactSize<T> {
    // Not pub since the caller needs to ensure that the count contract holds.
    fn new(iter: T, count: usize) -> Self {
        Self { iter, count }
    }
}

impl<T: Iterator> Iterator for ExactSize<T> {
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.count == 0 {
            debug_assert!(self.iter.next().is_none());
            None
        } else if let Some(v) = self.iter.next() {
            self.count -= 1; // checks for overflow in debug
            Some(v)
        } else {
            unreachable!();
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}

// Even though general TreeKey iterations may well be longer than usize::MAX
// we are sure that the aren't in this case since self.count <= usize::MAX
impl<T: Iterator> ExactSizeIterator for ExactSize<T> {}

impl<T: FusedIterator> FusedIterator for ExactSize<T> {}

// unsafe impl<T: Iterator> core::iter::TrustedLen for Counting<T> {}

/// A Keys wrapper that is is_empty()
pub struct Consume<'a>(KeysIter<Copied<Iter<'a, usize>>>);
impl<'a> Keys for Consume<'a> {
    fn next<M: KeyLookup + ?Sized>(&mut self) -> Result<usize, Traversal> {
        Keys::next::<M>(&mut self.0)
    }

    fn is_empty(&mut self) -> bool {
        true
    }
}
impl<'a> IntoKeys for Consume<'a> {
    type IntoKeys = Self;

    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

/// Node iterator
///
/// A managed indices state for iteration of nodes in a `TreeKey`.
///
/// `D` is the depth limit. Keys that are `Traversal::TooShort` (internal nodes)
/// will still be returned on iteration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeIter<M: ?Sized, const Y: usize, N, const D: usize = Y> {
    state: Indices<[usize; D]>,
    root: usize,
    depth: usize,
    _n: PhantomData<N>,
    _m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize, N, const D: usize> Default for NodeIter<M, Y, N, D> {
    fn default() -> Self {
        Self {
            state: Indices::default(),
            root: 0,
            // Marker to prevent initial index increment in `next()`
            depth: D + 1,
            _n: PhantomData,
            _m: PhantomData,
        }
    }
}

impl<M: TreeKey<Y> + ?Sized, const Y: usize, N, const D: usize> NodeIter<M, Y, N, D> {
    /// Limit and start iteration to at and below the provided root key.
    pub fn root<K: IntoKeys>(&mut self, root: K) -> Result<Node, Traversal> {
        let node = self.state.transcode::<M, Y, _>(root)?;
        self.root = node.depth();
        Ok(node)
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited.
    pub fn exact_size(self) -> ExactSize<Self> {
        assert!(self.depth > D);
        assert!(self.root == 0);
        debug_assert_eq!(&self.state, &Indices::default());
        assert!(D >= Y);
        ExactSize::new(self, M::metadata().count)
    }
}

impl<M, const Y: usize, N, const D: usize> Iterator for NodeIter<M, Y, N, D>
where
    M: TreeKey<Y> + ?Sized,
    N: Transcode + Default,
{
    type Item = Result<(N, Node), usize>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            debug_assert!(self.depth >= self.root);
            debug_assert!(self.depth <= D + 1);
            if self.depth == self.root {
                // Iteration done
                return None;
            }
            if self.depth <= D {
                // Not initial state: increment
                self.state[self.depth - 1] += 1;
            }
            return match M::transcode(Consume(self.state.iter().copied().into_keys())) {
                Err(Traversal::NotFound(depth)) => {
                    // Reset index at current depth, then retry with incremented index at depth - 1 or terminate
                    // Key lookup was performed and failed: depth is always >= 1
                    self.state[depth - 1] = 0;
                    self.depth = (depth - 1).max(self.root);
                    continue;
                }
                Ok((path, node)) => {
                    // Leaf or internal node found, save depth for increment at next iteration
                    self.depth = node.depth();
                    Some(Ok((path, node)))
                }
                Err(Traversal::TooShort(depth)) => {
                    // Target type can not hold keys
                    Some(Err(depth))
                }
                // TooLong: impossible due to Consume
                // Absent, Finalization, Invalid, Access: not returned by traverse(traverse_by_key())
                _ => unreachable!(),
            };
        }
    }
}

impl<M, const Y: usize, N, const D: usize> core::iter::FusedIterator for NodeIter<M, Y, N, D>
where
    M: TreeKey<Y> + ?Sized,
    N: Transcode + Default,
{
}
