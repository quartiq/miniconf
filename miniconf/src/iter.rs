use core::marker::PhantomData;

use crate::{DescendError, IntoKeys, KeyError, Keys, Schema, Short, Track, Transcode, TreeSchema};

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

// Even though general TreeSchema iterations may well be longer than usize::MAX
// we are sure that the aren't in this case since self.count <= usize::MAX
impl<T: Iterator> ExactSizeIterator for ExactSize<T> {}

// `Iterator` is sufficient to fuse
impl<T: Iterator> core::iter::FusedIterator for ExactSize<T> {}

// https://github.com/rust-lang/rust/issues/37572
// unsafe impl<T: Iterator> core::iter::TrustedLen for ExactSize<T> {}

/// Node iterator
///
/// A managed indices state for iteration of nodes `N` in a `TreeSchema`.
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
    /// Create a new iterator.
    ///
    /// # Panic
    /// If the root depth exceeds the state length.
    #[inline]
    pub const fn with(schema: &'static Schema, state: [usize; D], root: usize) -> Self {
        assert!(root <= D);
        Self {
            schema,
            state,
            root,
            // Marker to prevent initial index increment in `next()`
            depth: D + 1,
            _n: PhantomData,
        }
    }

    /// Create a new iterator with default root and initial state.
    #[inline]
    pub const fn new(schema: &'static Schema) -> Self {
        Self::with(schema, [0; D], 0)
    }

    /// Limit and start iteration to at and below the provided root key.
    ///
    /// This requires moving `self` to ensure `FusedIterator`.
    pub fn with_root(mut self, root: impl IntoKeys) -> Result<Self, DescendError<()>> {
        self.state = [0; D];
        let mut tr = Short::new(Track::new(&mut self.state[..]));
        tr.transcode(self.schema, root)?;
        self.root = tr.inner.depth;
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
    #[inline]
    pub const fn exact_size<T: TreeSchema + ?Sized>() -> ExactSize<Self> {
        if D < T::SHAPE.max_depth {
            panic!("insufficient depth for exact size iteration");
        }
        ExactSize {
            iter: Self::new(T::SCHEMA),
            count: T::SHAPE.count.get(),
        }
    }

    /// Return the current state
    #[inline]
    pub fn state(&self) -> Option<&[usize]> {
        self.state.get(..self.depth)
    }

    /// Return the root depth
    #[inline]
    pub const fn root(&self) -> usize {
        self.root
    }
}

impl<N: Transcode + Default, const D: usize> Iterator for NodeIter<N, D> {
    type Item = Result<N, N::Error>;

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
            let ret = path.transcode(self.schema, &mut idx);
            // Track() counts is the number of successful Keys::next()
            let depth = idx.depth;
            return match ret {
                Err(DescendError::Key(KeyError::NotFound)) => {
                    // Reset index at NotFound depth, then retry with incremented earlier index or terminate
                    self.state[depth] = 0;
                    self.depth = depth.max(self.root);
                    continue;
                }
                Err(DescendError::Key(KeyError::TooLong)) | Ok(()) => {
                    // Leaf node found, save depth for increment at next iteration
                    self.depth = depth;
                    Some(Ok(path))
                }
                Err(DescendError::Key(KeyError::TooShort)) => {
                    // Use Short<N> to also get internal short nodes
                    self.depth = depth;
                    continue;
                }
                Err(DescendError::Inner(e)) => {
                    // Target type can not hold keys
                    Some(Err(e))
                }
            };
        }
    }
}

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + Default, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}
