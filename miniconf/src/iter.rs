use core::marker::PhantomData;

use crate::{DescendError, IntoKeys, KeyError, Keys, Node, Schema, Track, Transcode};

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

impl<T> ExactSize<T> {
    /// Return a reference to the inner iterator
    #[inline]
    pub fn inner(&self) -> &T {
        &self.iter
    }
}

// Even though general TreeKey iterations may well be longer than usize::MAX
// we are sure that the aren't in this case since self.count <= usize::MAX
impl<T: Iterator> ExactSizeIterator for ExactSize<T> {}

// `Iterator` is sufficient to fuse
impl<T: Iterator> core::iter::FusedIterator for ExactSize<T> {}

// https://github.com/rust-lang/rust/issues/37572
// unsafe impl<T: Iterator> core::iter::TrustedLen for ExactSize<T> {}

/// Node iterator
///
/// A managed indices state for iteration of nodes `N` in a `TreeKey`.
///
/// `D` is the depth limit. Internal nodes will be returned on iteration where
/// the depth limit is exceeded.
///
/// The `Err(usize)` variant of the `Iterator::Item` indicates that `N` does
/// not have sufficient capacity and failed to encode the key at the given depth.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeIter<N, const D: usize> {
    // We can't use Packed as state since we need to be able to modify the
    // indices directly. Packed erases knowledge of the bit widths of the individual
    // indices.
    schema: &'static Schema,
    state: [usize; D],
    root: usize,
    depth: usize,
    _n: PhantomData<N>,
}

impl<N, const D: usize> NodeIter<N, D> {
    pub const fn new(schema: &'static Schema) -> Self {
        Self {
            schema,
            state: [0; D],
            root: 0,
            // Marker to prevent initial index increment in `next()`
            depth: D + 1,
            _n: PhantomData,
        }
    }

    /// Limit and start iteration to at and below the provided root key.
    ///
    /// This requires moving `self` to ensure `FusedIterator`.
    pub fn root(mut self, root: impl IntoKeys) -> Result<Self, DescendError<()>> {
        let mut tr = Track::from(&mut self.state[..]);
        tr.transcode(self.schema, root)?;
        self.root = tr.node().depth;
        self.depth = D + 1;
        Ok(self)
    }

    /// Wrap the iterator in an exact size counting iterator that is
    /// `FusedIterator` and `ExactSizeIterator`.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited or if the iteration root
    /// is not the tree root.
    ///
    // TODO: improve by e.g. changing schema
    pub fn exact_size(self) -> ExactSize<Self> {
        assert_eq!(self.depth, D + 1, "NodeIter partially consumed");
        assert_eq!(self.root, 0, "NodeIter on sub-tree");
        debug_assert_eq!(&self.state, &[0; D]); // ensured by depth = D + 1 marker and contract
        let meta = self.schema.shape();
        assert!(
            D >= meta.max_depth,
            "depth D = {D} must be at least {}",
            meta.max_depth
        );
        ExactSize {
            iter: self,
            count: meta.count.get(),
        }
    }

    /// Return the current iteration depth
    pub const fn current_depth(&self) -> usize {
        self.depth
    }

    /// Return the root depth
    pub const fn root_depth(&self) -> usize {
        self.root
    }
}

impl<N, const D: usize> Iterator for NodeIter<N, D>
where
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
            let mut path = N::default();
            let mut idx = self.state.iter().into_keys().track();
            let ret = path.transcode(&self.schema, &mut idx);
            let node = idx.node();
            return match ret {
                Err(DescendError::Key(KeyError::NotFound)) => {
                    // Reset index at NotFound depth, then retry with incremented earlier index or terminate
                    // Track() counts is the number of successful Keys::next()
                    self.state[node.depth] = 0;
                    self.depth = node.depth.max(self.root);
                    continue;
                }
                Err(DescendError::Key(KeyError::TooLong | KeyError::TooShort)) | Ok(()) => {
                    // Leaf or internal node found, save depth for increment at next iteration
                    self.depth = node.depth;
                    Some(Ok((path, node)))
                }
                Err(DescendError::Inner(_)) => {
                    // Target type can not hold keys
                    Some(Err(node.depth))
                }
            };
        }
    }
}

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + Default, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}
