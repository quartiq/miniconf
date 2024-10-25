use core::marker::PhantomData;

use crate::{IntoKeys, KeyLookup, Keys, Metadata, Node, Transcode, Traversal, TreeKey};

/// Counting wrapper for iterators with known exact size
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactSize<T> {
    iter: T,
    count: usize,
}

impl<T: Iterator> Iterator for ExactSize<T> {
    type Item = T::Item;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(v) = self.iter.next() {
            self.count -= 1; // checks for overflow in debug
            Some(v)
        } else {
            debug_assert!(self.count == 0);
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}

// Even though general TreeKey iterations may well be longer than usize::MAX
// we are sure that the aren't in this case since self.count <= usize::MAX
impl<T: Iterator> ExactSizeIterator for ExactSize<T> {}

// `Iterator` is sufficient to fuse
impl<T: Iterator> core::iter::FusedIterator for ExactSize<T> {}

// https://github.com/rust-lang/rust/issues/37572
// unsafe impl<T: Iterator> core::iter::TrustedLen for ExactSize<T> {}

/// A Keys wrapper that can always finalize()
struct Consume<T>(T);
impl<T: Keys> Keys for Consume<T> {
    #[inline]
    fn next(&mut self, lookup: &KeyLookup) -> Result<usize, Traversal> {
        self.0.next(lookup)
    }

    #[inline]
    fn finalize(&mut self) -> Result<(), Traversal> {
        Ok(())
    }
}
impl<T: Keys> IntoKeys for Consume<T> {
    type IntoKeys = Self;

    #[inline]
    fn into_keys(self) -> Self::IntoKeys {
        self
    }
}

/// Node iterator
///
/// A managed indices state for iteration of nodes `N` in a `TreeKey`.
///
/// `D` is the depth limit. Internal nodes will be returned on iteration where
/// the depth limit is exceeded.
///
/// The `Err(usize)` variant of the `Iterator::Item` indicates that `N` does
/// not have sufficient capacity and failed to encode the key at the given depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeIter<M: ?Sized, N, const D: usize> {
    // We can't use Packed as state since we need to be able to modify the
    // indices directly. Packed erases knowledge of the bit widths of the individual
    // indices.
    state: [usize; D],
    root: usize,
    depth: usize,
    _n: PhantomData<N>,
    _m: PhantomData<M>,
}

impl<M: ?Sized, N, const D: usize> Default for NodeIter<M, N, D> {
    fn default() -> Self {
        Self {
            state: [0; D],
            root: 0,
            // Marker to prevent initial index increment in `next()`
            depth: D + 1,
            _n: PhantomData,
            _m: PhantomData,
        }
    }
}

impl<M: TreeKey + ?Sized, N, const D: usize> NodeIter<M, N, D> {
    /// Limit and start iteration to at and below the provided root key.
    ///
    /// This requires moving `self` to ensure `FusedIterator`.
    pub fn root<K: IntoKeys>(mut self, root: K) -> Result<Self, Traversal> {
        let node = self.state.transcode::<M, _>(root)?;
        self.root = node.depth();
        self.depth = D + 1;
        Ok(self)
    }

    /// Wrap the iterator in an exact size counting iterator that is
    /// `FusedIterator` and `ExactSizeIterator`.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited.
    pub fn exact_size(self) -> ExactSize<Self> {
        assert_eq!(self.depth, D + 1, "NodeIter partially consumed");
        assert_eq!(self.root, 0, "NodeIter on sub-tree");
        debug_assert_eq!(&self.state, &[0; D]); // ensured by depth = D + 1 marker
        let meta = M::traverse_all::<Metadata>().unwrap();
        assert!(
            D >= meta.max_depth,
            "depth D = {D} must be at least {}",
            meta.max_depth
        );
        ExactSize {
            iter: self,
            count: meta.count,
        }
    }
}

impl<M, N, const D: usize> Iterator for NodeIter<M, N, D>
where
    M: TreeKey + ?Sized,
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
            return match M::transcode(Consume(self.state.into_keys())) {
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
                // Absent, Finalization, Invalid, Access: not returned by transcode (traverse_by_key())
                _ => unreachable!(),
            };
        }
    }
}

/// Do not allow manipulation of `depth` other than through iteration .
impl<M: TreeKey + ?Sized, N: Transcode + Default, const D: usize> core::iter::FusedIterator
    for NodeIter<M, N, D>
{
}
