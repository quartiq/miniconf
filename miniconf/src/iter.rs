use core::marker::PhantomData;

use crate::{DescendError, Internal, IntoKeys, KeyError, Schema, Transcode};

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
/// A managed indices state for iteration of leaf nodes `N` in a `TreeSchema`.
///
/// `D` is the depth limit. Leaf nodes deeper than `D` are skipped.
///
/// The `Err(N::Error)` variant of the `Iterator::Item` indicates that `N`
/// failed to encode a yielded node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeIter<N, const D: usize> {
    // We can't use Packed as state since we need to be able to modify the
    // indices directly. Packed erases knowledge of the bit widths of the individual
    // indices.
    root_schema: &'static Schema,
    root_depth: usize,
    parents: [Option<&'static Internal>; D],
    indices: [usize; D],
    schema: &'static Schema,
    depth: usize,
    target: PhantomData<N>,
}

impl<N, const D: usize> NodeIter<N, D> {
    /// Create a new iterator.
    pub const fn new(root_schema: &'static Schema) -> Self {
        Self {
            root_schema,
            root_depth: 0,
            parents: [None; D],
            indices: [0; D],
            schema: root_schema,
            depth: D + 1,
            target: PhantomData,
        }
    }

    fn rooted(
        root_schema: &'static Schema,
        indices: [usize; D],
        root_depth: usize,
        schema: &'static Schema,
    ) -> Self {
        let mut iter = Self {
            root_schema,
            root_depth,
            parents: [None; D],
            indices,
            schema,
            depth: D + 1,
            target: PhantomData,
        };
        iter.fill_parents(root_depth);
        iter
    }

    /// Limit and start iteration from the provided root key.
    ///
    /// If the selected root is itself a leaf node, it is returned first. Otherwise iteration
    /// continues below that root.
    ///
    /// This requires moving `self` to ensure `FusedIterator`.
    pub fn with_root(
        schema: &'static Schema,
        root: impl IntoKeys,
    ) -> Result<Self, DescendError<()>> {
        let mut indices = [0; D];
        let info = schema
            .resolve_into(root, indices.as_mut())
            .map_err(|err| err.error)?;
        Ok(Self::rooted(schema, indices, info.depth, info.schema))
    }

    /// Wrap the iterator in an exact size counting iterator that is
    /// `FusedIterator` and `ExactSizeIterator`.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited or if the iteration root
    /// is not the tree root.
    ///
    pub const fn exact_size(self) -> ExactSize<Self> {
        let shape = self.root_schema.shape();
        if D < shape.max_depth {
            panic!("insufficient depth for exact size iteration");
        }
        let mut i = 0;
        while i < D {
            if self.indices[i] != 0 {
                panic!("exact size requires a fresh root iterator");
            }
            i += 1;
        }
        if self.root_depth != 0 || self.depth != D + 1 {
            panic!("exact size requires a fresh root iterator");
        }
        ExactSize {
            iter: self,
            count: shape.count.get(),
        }
    }

    /// Return the schema tree being iterated.
    pub const fn root_schema(&self) -> &'static Schema {
        self.root_schema
    }

    /// Return the current yielded key indices.
    pub fn indices(&self) -> Option<&[usize]> {
        self.indices.get(..self.depth)
    }

    /// Return the current yielded leaf schema.
    pub fn schema(&self) -> Option<&'static Schema> {
        (self.depth <= D).then_some(self.schema)
    }

    /// Return the selected subtree root depth.
    pub const fn root_depth(&self) -> usize {
        self.root_depth
    }

    fn fill_parents(&mut self, depth: usize) -> &'static Schema {
        let mut schema = self.root_schema;
        let mut i = 0;
        while i < depth {
            let Some(internal) = schema.internal() else {
                break;
            };
            schema = internal.get_schema(self.indices[i]);
            self.parents[i] = Some(internal);
            i += 1;
        }
        schema
    }

    fn descend_leftmost(&mut self) {
        let mut schema = self.schema;
        while self.depth < D {
            let Some(internal) = schema.internal() else {
                break;
            };
            self.parents[self.depth] = Some(internal);
            self.indices[self.depth] = 0;
            schema = internal.get_schema(0);
            self.depth += 1;
        }
        self.schema = schema;
    }

    fn bump(&mut self) -> bool {
        let mut depth = self.depth;
        while depth > self.root_depth {
            let parent = depth - 1;
            let Some(internal) = self.parents[parent] else {
                depth = parent;
                continue;
            };
            let next = self.indices[parent] + 1;
            if next < internal.len().get() {
                self.indices[parent] = next;
                self.depth = parent + 1;
                self.schema = internal.get_schema(next);
                return true;
            }
            depth = parent;
        }
        self.depth = self.root_depth;
        false
    }
}

impl<N: Transcode + Default, const D: usize> Iterator for NodeIter<N, D> {
    type Item = Result<N, N::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            debug_assert!(self.depth >= self.root_depth);
            debug_assert!(self.depth <= D + 1);
            if self.depth == self.root_depth {
                return None;
            }
            if self.depth <= D {
                if !self.bump() {
                    return None;
                }
            } else {
                self.depth = self.root_depth;
            }

            self.descend_leftmost();
            debug_assert!(self.depth >= self.root_depth);
            debug_assert!(self.depth <= D);
            let mut item = N::default();
            match item.transcode_from(self.root_schema, &self.indices[..self.depth]) {
                Ok(()) => return Some(Ok(item)),
                Err(DescendError::Key(KeyError::TooShort)) => {}
                Err(DescendError::Inner(e)) => return Some(Err(e)),
                Err(DescendError::Key(KeyError::NotFound | KeyError::TooLong)) => unreachable!(),
            }
        }
    }
}

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + Default, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}
