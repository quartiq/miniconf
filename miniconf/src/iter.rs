use crate::{DescendError, FromConfig, Internal, IntoKeys, KeyError, Meta, Schema, Transcode};

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

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct TreeCursor<const D: usize> {
    // We can't use Packed as state since we need to be able to modify the
    // indices directly. Packed erases knowledge of the bit widths of the individual
    // indices.
    schema: &'static Schema,
    state: [usize; D],
    root: usize,
    depth: usize,
}

impl<const D: usize> TreeCursor<D> {
    /// Create a new traversal cursor.
    ///
    /// # Panic
    /// If the root depth exceeds the state length.
    pub const fn new(schema: &'static Schema, state: [usize; D], root: usize) -> Self {
        assert!(root <= D);
        Self {
            schema,
            state,
            root,
            // Marker to prevent initial increment in `next()`.
            depth: D + 1,
        }
    }

    /// Create a traversal cursor positioned at the root.
    ///
    /// # Panic
    /// If the root depth exceeds the state length.
    pub const fn at_root(schema: &'static Schema, state: [usize; D], root: usize) -> Self {
        assert!(root <= D);
        Self {
            schema,
            state,
            root,
            depth: root,
        }
    }

    /// Return the underlying schema.
    pub const fn schema(&self) -> &'static Schema {
        self.schema
    }

    /// Return the current state.
    pub fn state(&self) -> Option<&[usize]> {
        (self.depth <= D).then(|| &self.state[..self.depth])
    }

    /// Return the root depth.
    pub const fn root(&self) -> usize {
        self.root
    }

    /// Return the current schema node.
    pub fn current_schema(&self) -> &'static Schema {
        self.schema.get_indexed(&self.state[..self.depth])
    }

    /// Descend one level to the first child.
    pub fn descend(&mut self) -> bool {
        let schema = self.current_schema();
        if schema.is_leaf() || self.depth >= D || schema.is_empty() {
            return false;
        }
        self.state[self.depth] = 0;
        self.depth += 1;
        true
    }

    /// Descend to the leftmost reachable node.
    pub fn descend_leftmost(&mut self) {
        while self.descend() {}
    }

    /// Bump the current node to the next sibling in depth-first order.
    pub fn bump(&mut self) -> bool {
        let mut depth = self.depth;
        while depth > self.root {
            let parent = depth - 1;
            let internal = self
                .schema
                .get_indexed(&self.state[..parent])
                .internal
                .as_ref()
                .unwrap();
            let next = self.state[parent] + 1;
            if next < internal.len().get() {
                self.state[parent] = next;
                self.depth = parent + 1;
                return true;
            }
            depth = parent;
        }
        self.depth = self.root;
        false
    }

    /// Advance to the next node in preorder traversal.
    pub fn advance_preorder(&mut self) -> bool {
        if self.descend() || self.bump() {
            return true;
        }
        self.depth = D + 1;
        false
    }
}

/// Node iterator
///
/// A managed indices state for iteration of leaf nodes `N` in a `TreeSchema`.
///
/// `D` is the depth limit. Leaf nodes deeper than `D` are skipped.
///
/// The `Err(N::Error)` variant of the `Iterator::Item` indicates that `N`
/// failed to encode a yielded node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeIter<N: FromConfig, const D: usize> {
    cursor: TreeCursor<D>,
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
        Self {
            cursor: TreeCursor::new(schema, state, root),
            config,
        }
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
        config: N::Config,
    ) -> Result<Self, DescendError<()>> {
        let mut state = [0; D];
        let info = schema
            .resolve_into(root, state.as_mut())
            .map_err(|err| err.error)?;
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
        let shape = self.cursor.schema.shape();
        if D < shape.max_depth {
            panic!("insufficient depth for exact size iteration");
        }
        let mut i = 0;
        while i < D {
            if self.cursor.state[i] != 0 {
                panic!("exact size requires a fresh root iterator");
            }
            i += 1;
        }
        if self.cursor.root != 0 || self.cursor.depth != D + 1 {
            panic!("exact size requires a fresh root iterator");
        }
        ExactSize {
            iter: self,
            count: shape.count.get(),
        }
    }

    /// Return the underlying schema
    pub const fn schema(&self) -> &'static Schema {
        self.cursor.schema()
    }

    /// Return the current state
    pub fn state(&self) -> Option<&[usize]> {
        self.cursor.state()
    }

    /// Return the root depth
    pub const fn root(&self) -> usize {
        self.cursor.root()
    }
}

impl<N: Transcode + FromConfig, const D: usize> Iterator for NodeIter<N, D> {
    type Item = Result<N, N::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let cursor = &mut self.cursor;
            debug_assert!(cursor.depth >= cursor.root);
            debug_assert!(cursor.depth <= D + 1);
            if cursor.depth == cursor.root {
                return None;
            }
            if cursor.depth <= D {
                if !cursor.bump() {
                    return None;
                }
            } else {
                cursor.depth = cursor.root;
            }

            cursor.descend_leftmost();
            debug_assert!(cursor.depth >= cursor.root);
            debug_assert!(cursor.depth <= D);
            let mut item = N::from_config(&self.config);
            match item.transcode_from(cursor.schema, &cursor.state[..cursor.depth]) {
                Ok(()) => return Some(Ok(item)),
                Err(DescendError::Key(KeyError::TooShort)) => {}
                Err(DescendError::Inner(e)) => return Some(Err(e)),
                Err(DescendError::Key(KeyError::NotFound | KeyError::TooLong)) => unreachable!(),
            }
        }
    }
}

// Contract: Do not allow manipulation of `depth` other than through iteration.
impl<N: Transcode + FromConfig, const D: usize> core::iter::FusedIterator for NodeIter<N, D> {}

/// Schema iterator
///
/// A managed indices state for iteration of all nodes in a `TreeSchema`.
///
/// `D` is the depth limit. Nodes deeper than `D` are skipped.
///
/// The yielded metadata is the child-edge metadata when present, otherwise the
/// node metadata.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SchemaLevel {
    schema: &'static Schema,
    meta: Option<Meta>,
}

/// One schema node yielded during preorder traversal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SchemaEntry<const D: usize> {
    schema: &'static Schema,
    meta: Option<Meta>,
    state: [usize; D],
    depth: usize,
}

impl<const D: usize> SchemaEntry<D> {
    /// The schema node at the current traversal position.
    pub const fn schema(&self) -> &'static Schema {
        self.schema
    }

    /// The child-edge metadata when present, otherwise the node metadata.
    pub const fn meta(&self) -> Option<Meta> {
        self.meta
    }

    /// The traversal state buffer.
    pub const fn state(&self) -> [usize; D] {
        self.state
    }

    /// The active prefix length within [`Self::state`].
    pub const fn depth(&self) -> usize {
        self.depth
    }

    /// The active traversal prefix.
    pub fn path(&self) -> &[usize] {
        &self.state[..self.depth]
    }
}

/// A managed indices state for iteration of all schema nodes in preorder.
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaIter<const D: usize> {
    cursor: TreeCursor<D>,
    root: SchemaLevel,
    levels: [Option<SchemaLevel>; D],
}

impl<const D: usize> SchemaIter<D> {
    /// Create a new iterator.
    ///
    /// # Panic
    /// If the root depth exceeds the state length.
    pub fn new(schema: &'static Schema, state: [usize; D], root: usize) -> Self {
        let root_level = SchemaLevel {
            schema,
            meta: schema.meta,
        };
        let mut levels = [const { None }; D];
        let mut level = root_level;
        let mut depth = 0;
        while depth < root {
            level = Self::child_level(level, state[depth]);
            levels[depth] = Some(level);
            depth += 1;
        }
        Self {
            cursor: TreeCursor::at_root(schema, state, root),
            root: root_level,
            levels,
        }
    }

    /// Limit and start iteration from the provided root key.
    pub fn with_root(
        schema: &'static Schema,
        root: impl IntoKeys,
    ) -> Result<Self, DescendError<()>> {
        let mut state = [0; D];
        let info = schema
            .resolve_into(root, state.as_mut())
            .map_err(|err| err.error)?;
        Ok(Self::new(schema, state, info.depth))
    }

    fn child_level(parent: SchemaLevel, index: usize) -> SchemaLevel {
        match parent.schema.internal.as_ref().unwrap() {
            Internal::Named(children) => {
                let child = &children[index];
                SchemaLevel {
                    schema: child.schema,
                    meta: child.meta.or(child.schema.meta),
                }
            }
            Internal::Numbered(children) => {
                let child = &children[index];
                SchemaLevel {
                    schema: child.schema,
                    meta: child.meta.or(child.schema.meta),
                }
            }
            Internal::Homogeneous(child) => SchemaLevel {
                schema: child.schema,
                meta: child.meta.or(child.schema.meta),
            },
        }
    }

    fn current_level(&self) -> SchemaLevel {
        match self.cursor.depth {
            0 => self.root,
            depth if depth <= D => self.levels[depth - 1].unwrap(),
            _ => unreachable!(),
        }
    }

    fn sync_current_level(&mut self) {
        let depth = self.cursor.depth;
        if depth == 0 || depth > D {
            return;
        }
        let parent = if depth == 1 {
            self.root
        } else {
            self.levels[depth - 2].unwrap()
        };
        self.levels[depth - 1] = Some(Self::child_level(parent, self.cursor.state[depth - 1]));
    }
}

impl<const D: usize> Iterator for SchemaIter<D> {
    type Item = SchemaEntry<D>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor.depth > D {
            return None;
        }
        let current = self.current_level();
        let mut state = [0; D];
        let depth = self.cursor.state().map_or(0, <[usize]>::len);
        if let Some(current) = self.cursor.state() {
            state[..depth].copy_from_slice(current);
        }
        if self.cursor.advance_preorder() {
            self.sync_current_level();
        }
        Some(SchemaEntry {
            schema: current.schema,
            meta: current.meta,
            state,
            depth,
        })
    }
}

impl<const D: usize> core::iter::FusedIterator for SchemaIter<D> {}
