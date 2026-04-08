use crate::{DescendError, FromConfig, IntoKeys, KeyError, Schema, Track, Transcode};

/// Counting wrapper for iterators with known exact size
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExactSize<T> {
    iter: T,
    count: usize,
}

impl<T: Iterator> Iterator for ExactSize<T> {
    type Item = T::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(v) = self.iter.next() {
            self.count -= 1; // checks for overflow in debug
            Some(v)
        } else {
            debug_assert!(self.count == 0);
            None
        }
    }

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
/// `D` is the depth limit. If the chosen output representation can encode
/// non-leaf nodes, those may be returned where the depth limit is exceeded.
///
/// The `Err(usize)` variant of the `Iterator::Item` indicates that `N` does
/// not have sufficient capacity and failed to encode the key at the given depth.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeIter<N: FromConfig, const D: usize> {
    // We can't use Packed as state since we need to be able to modify the
    // indices directly. Packed erases knowledge of the bit widths of the individual
    // indices.
    schema: &'static Schema,
    state: [usize; D],
    root: usize,
    depth: usize,
    config: N::Config,
}

impl<N: FromConfig, const D: usize> NodeIter<N, D> {
    /// Create a new iterator.
    ///
    /// # Panic
    /// If the root depth exceeds the state length.
    pub const fn new(
        schema: &'static Schema,
        state: [usize; D],
        root: usize,
        config: N::Config,
    ) -> Self {
        assert!(root <= D);
        Self {
            schema,
            state,
            root,
            // Marker to prevent initial index increment in `next()`
            depth: D + 1,
            config,
        }
    }

    /// Limit and start iteration from the provided root key.
    ///
    /// If the selected root is itself yielded by the iterator (because it is already a leaf or
    /// because the depth limit stops there), it is returned first. Otherwise iteration continues
    /// below that root.
    ///
    /// This requires moving `self` to ensure `FusedIterator`.
    pub fn with_root(
        schema: &'static Schema,
        root: impl IntoKeys,
        config: N::Config,
    ) -> Result<Self, DescendError<()>> {
        let mut state = [0; D];
        let info = schema.node_info_with_state(root, Some(state.as_mut()))?;
        Ok(Self::new(schema, state, info.depth, config))
    }

    /// Wrap the iterator in an exact size counting iterator that is
    /// `FusedIterator` and `ExactSizeIterator`.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited or if the iteration root
    /// is not the tree root.
    ///
    pub const fn exact_size(self) -> ExactSize<Self> {
        let shape = self.schema.shape();
        if D < shape.max_depth {
            panic!("insufficient depth for exact size iteration");
        }
        let mut i = 0;
        while i < D {
            if self.state[i] != 0 {
                panic!("exact size requires a fresh root iterator");
            }
            i += 1;
        }
        if self.root != 0 || self.depth != D + 1 {
            panic!("exact size requires a fresh root iterator");
        }
        ExactSize {
            iter: self,
            count: shape.count.get(),
        }
    }

    /// Return the underlying schema
    pub const fn schema(&self) -> &'static Schema {
        self.schema
    }

    /// Return the current state
    pub fn state(&self) -> Option<&[usize]> {
        self.state.get(..self.depth)
    }

    /// Return the root depth
    pub const fn root(&self) -> usize {
        self.root
    }
}

impl<N: Transcode + FromConfig, const D: usize> Iterator for NodeIter<N, D> {
    type Item = Result<N, N::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            debug_assert!(self.depth >= self.root);
            debug_assert!(self.depth <= self.state.len() + 1);
            if self.depth == self.root {
                return None;
            }
            if self.depth <= self.state.len() {
                self.state[self.depth - 1] += 1;
            }
            let mut item = Track::new(N::from_config(&self.config));
            let ret = item.transcode_from(self.schema, &self.state[..]);
            let (item, depth) = item.into_inner();
            match ret {
                Err(DescendError::Key(KeyError::NotFound)) => {
                    self.state[depth] = 0;
                    self.depth = depth.max(self.root);
                }
                Err(DescendError::Key(KeyError::TooLong)) | Ok(()) => {
                    self.depth = depth;
                    return Some(Ok(item));
                }
                Err(DescendError::Key(KeyError::TooShort)) => {
                    self.depth = depth;
                }
                Err(DescendError::Inner(e)) => {
                    self.depth = depth;
                    return Some(Err(e));
                }
            }
        }
    }
}

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + FromConfig, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}
