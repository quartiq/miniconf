use core::marker::PhantomData;

use crate::{DescendError, Internal, IntoKeys, KeyError, Keys, Schema, Transcode};

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

/// A Keys wrapper that can always finalize()
pub(crate) struct Consume<T>(pub(crate) T);
impl<T: Keys> Keys for Consume<T> {
    #[inline]
    fn next(&mut self, internal: &Internal) -> Result<usize, KeyError> {
        self.0.next(internal)
    }

    #[inline]
    fn finalize(&mut self) -> bool {
        true
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
    pub fn new(schema: &'static Schema) -> Self {
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
    pub fn root<K: IntoKeys>(mut self, root: K) -> Result<Self, DescendError> {
        let mut root = root.into_keys().track();
        self.state.transcode(self.schema, &mut root)?;
        self.root = root.count();
        self.depth = D + 1;
        Ok(self)
    }

    /// Wrap the iterator in an exact size counting iterator that is
    /// `FusedIterator` and `ExactSizeIterator`.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited or if the iteration root
    /// is not the tree root.
    pub fn exact_size(self) -> ExactSize<Self> {
        assert_eq!(self.depth, D + 1, "NodeIter partially consumed");
        assert_eq!(self.root, 0, "NodeIter on sub-tree");
        debug_assert_eq!(&self.state, &[0; D]); // ensured by depth = D + 1 marker and contract
        let meta = self.schema.metadata();
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
    pub fn current_depth(&self) -> usize {
        self.depth
    }

    /// Return the root depth
    pub fn root_depth(&self) -> usize {
        self.root
    }
}

impl<N, const D: usize> Iterator for NodeIter<N, D>
where
    N: Transcode + Default,
{
    type Item = Result<(N, (usize, bool)), usize>;

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
            let mut idx = Consume(self.state.iter().into_keys()).track();
            let ret = path.transcode(&self.schema, &mut idx);
            let depth = idx.count();
            let leaf = idx.done();
            return match ret {
                Err(DescendError::Key(KeyError::NotFound)) => {
                    // Reset index at current depth, then retry with incremented index at depth - 1 or terminate
                    // Key lookup was performed and failed: depth is always >= 1
                    self.state[depth - 1] = 0;
                    self.depth = (depth - 1).max(self.root);
                    continue;
                }
                Ok(()) => {
                    // Leaf or internal node found, save depth for increment at next iteration
                    self.depth = depth;
                    Some(Ok((path, (depth, leaf))))
                }
                Err(DescendError::Inner) => {
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

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + Default, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}
