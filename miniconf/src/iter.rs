use core::marker::PhantomData;

use crate::{DescendError, IntoKeys, KeyError, Keys, Schema, Short, Track, Transcode};

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
    pub fn with_root(
        schema: &'static Schema,
        root: impl IntoKeys,
    ) -> Result<Self, DescendError<()>> {
        let mut state = [0; D];
        let mut root = root.into_keys().track();
        let mut tr = Short::new(state.as_mut());
        tr.transcode(schema, &mut root)?;
        Ok(Self::with(schema, state, root.depth()))
    }

    /// Wrap the iterator in an exact size counting iterator that is
    /// `FusedIterator` and `ExactSizeIterator`.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited or if the iteration root
    /// is not the tree root.
    ///
    #[inline]
    pub const fn exact_size(schema: &'static Schema) -> ExactSize<Self> {
        let shape = schema.shape();
        if D < shape.max_depth {
            panic!("insufficient depth for exact size iteration");
        }
        ExactSize {
            iter: Self::new(schema),
            count: shape.count.get(),
        }
    }

    /// Return the underlying schema
    #[inline]
    pub const fn schema(&self) -> &'static Schema {
        self.schema
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
            let mut item = Track::new(N::default());
            let ret = item.transcode(self.schema, &self.state[..]);
            // Track<N> counts is the number of successful Keys::next()
            let (item, depth) = item.into_inner();
            match ret {
                Err(DescendError::Key(KeyError::NotFound)) => {
                    // Reset index at NotFound depth, then retry with incremented earlier index or terminate
                    self.state[depth] = 0;
                    self.depth = depth.max(self.root);
                }
                Err(DescendError::Key(KeyError::TooLong)) | Ok(()) => {
                    // Leaf node found, save depth for increment at next iteration
                    self.depth = depth;
                    return Some(Ok(item));
                }
                Err(DescendError::Key(KeyError::TooShort)) => {
                    // Use Short<N> to suppress this branch and also get internal short nodes
                    self.depth = depth;
                }
                Err(DescendError::Inner(e)) => {
                    // Target type can not hold keys
                    return Some(Err(e));
                }
            }
        }
    }
}

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + Default, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}
