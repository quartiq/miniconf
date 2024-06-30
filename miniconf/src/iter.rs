use crate::{
    Error, IntoKeys, KeyLookup, Keys, KeysIter, Node, NodeLookup, Packed, Traversal, TreeKey,
};
use core::{
    fmt::Write,
    iter::{Copied, FusedIterator},
    marker::PhantomData,
    slice::Iter,
};

/// Counting wrapper for iterators with known size
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counting<T> {
    iter: T,
    count: usize,
}

impl<T> Counting<T> {
    // Not pub since the caller needs to ensure that the count contract holds.
    fn new(iter: T, count: usize) -> Self {
        Self { iter, count }
    }
}

impl<T: Iterator> Iterator for Counting<T> {
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
impl<T: Iterator> ExactSizeIterator for Counting<T> {}

impl<T: FusedIterator> FusedIterator for Counting<T> {}

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

/// A managed indices state for iteration of nodes in a `TreeKey`.
///
/// `D` is the depth limit. Keys that are `Traversal::TooShort` (internal nodes)
/// will still be returned on iteration.
#[derive(Clone, Debug, PartialEq, Eq)]
struct State<const D: usize> {
    state: [usize; D],
    depth: usize,
    root: usize,
}

impl<const D: usize> Default for State<D> {
    fn default() -> Self {
        Self {
            state: [0; D],
            // Marker to prevent initial index increment in `next()`
            depth: D + 1,
            root: 0,
        }
    }
}

impl<const D: usize> State<D> {
    /// Create a new iterator state from the given root indices.
    fn new(root: &[usize]) -> Result<Self, Traversal> {
        let mut state = [0; D];
        if root.len() > state.len() {
            return Err(Traversal::TooLong(state.len()));
        }
        state[..root.len()].copy_from_slice(root);
        Ok(Self {
            state,
            root: root.len(),
            ..Default::default()
        })
    }

    /// Try to prepare for the next iteratiion
    ///
    /// Increment current index and return indices iterator.
    fn next(&mut self) -> Option<impl IntoKeys + '_> {
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
        Some(Consume(self.state.iter().copied().into_keys()))
    }

    /// Handle the result of a `traverse_by_key()` and update `depth` for next iteration.
    fn handle<E>(&mut self, ret: Result<usize, Error<E>>) -> Option<Result<usize, (usize, E)>> {
        match ret {
            Err(Error::Traversal(Traversal::NotFound(depth))) => {
                // Reset index at current depth, then retry with incremented index at depth - 1 or terminate
                // Key lookup was performed and failed: depth is always >= 1
                self.state[depth - 1] = 0;
                self.depth = (depth - 1).max(self.root);
                None
            }
            Ok(depth)
            | Err(Error::Traversal(Traversal::TooShort(depth) | Traversal::TooLong(depth))) => {
                debug_assert!(depth >= self.root);
                // Leaf or internal node found, save depth for increment at next iteration
                self.depth = depth;
                Some(Ok(depth))
            }
            Err(Error::Inner(depth, err)) => Some(Err((depth, err))),
            // Absent, Finalization, Invalid, Access: not returned by traverse_by_key()
            _ => unreachable!(),
        }
    }
}

/// Node iterator
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeIter<M: ?Sized, const Y: usize, N, const D: usize = Y> {
    state: [usize; D],
    root: usize,
    depth: usize,
    _n: PhantomData<N>,
    _m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize, N, const D: usize> Default for NodeIter<M, Y, N, D> {
    fn default() -> Self {
        Self {
            state: [0; D],
            root: 0,
            depth: D + 1,
            _n: PhantomData,
            _m: PhantomData,
        }
    }
}

impl<M: TreeKey<Y> + ?Sized, const Y: usize, N, const D: usize> NodeIter<M, Y, N, D> {
    /// Limit and start iteration to at and below the provided root key.
    pub fn root<K: IntoKeys>(&mut self, root: K) -> Result<Node, Traversal> {
        let node = self.state.lookup::<M, Y, _>(root)?;
        self.root = node.depth();
        Ok(node)
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited.
    pub fn count(self) -> Counting<Self> {
        assert!(self.depth > D);
        assert!(self.root == 0);
        assert!(D >= Y);
        Counting::new(self, M::metadata().count)
    }
}

impl<M, const Y: usize, N, const D: usize> Iterator for NodeIter<M, Y, N, D>
where
    M: TreeKey<Y> + ?Sized,
    N: NodeLookup + Default,
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
            let keys = Consume(self.state.iter().copied().into_keys());
            let mut path = N::default();
            return match path.lookup::<M, Y, _>(keys) {
                Err(Traversal::NotFound(depth)) => {
                    // Reset index at current depth, then retry with incremented index at depth - 1 or terminate
                    // Key lookup was performed and failed: depth is always >= 1
                    self.state[depth - 1] = 0;
                    self.depth = (depth - 1).max(self.root);
                    continue;
                }
                Ok(node) => {
                    debug_assert!(node.depth() >= self.root);
                    // Leaf or internal node found, save depth for increment at next iteration
                    self.depth = node.depth();
                    Some(Ok((path, node)))
                }
                Err(Traversal::TooShort(depth)) => Some(Err(depth)),
                // TooLong: Consume
                // TooShort: Absent, Finalization, Invalid, Access: not returned by traverse_by_key()
                _ => unreachable!(),
            };
        }
    }
}

impl<M, const Y: usize, N, const D: usize> core::iter::FusedIterator for NodeIter<M, Y, N, D>
where
    M: TreeKey<Y> + ?Sized,
    N: NodeLookup + Default,
{
}

/// An iterator over the paths in a `TreeKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathIter<'a, M: ?Sized, const Y: usize, P: ?Sized, const D: usize> {
    state: State<D>,
    separator: &'a str,
    _p: PhantomData<P>,
    _m: PhantomData<M>,
}

impl<'a, M: TreeKey<Y> + ?Sized, const Y: usize, P: ?Sized, const D: usize>
    PathIter<'a, M, Y, P, D>
{
    /// Create a new iterator given a path hierarchy separator.
    pub fn new(separator: &'a str) -> Self {
        Self {
            state: State::default(),
            separator,
            _p: PhantomData,
            _m: PhantomData,
        }
    }

    /// Limit and start iteration to at and below the provided root key.
    pub fn root<K: IntoKeys>(&mut self, root: K) -> Result<usize, Traversal> {
        let (idx, depth) = M::indices(root)?;
        self.state = State::new(&idx[..depth])?;
        Ok(depth)
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited.
    pub fn count(self) -> Counting<Self> {
        assert!(self.state.depth > D);
        assert!(self.state.root == 0);
        assert!(D >= Y);
        Counting::new(self, M::metadata().count)
    }
}

impl<'a, M, const Y: usize, P, const D: usize> Iterator for PathIter<'a, M, Y, P, D>
where
    M: TreeKey<Y> + ?Sized,
    P: Write + Default + ?Sized,
{
    type Item = Result<P, core::fmt::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let keys = self.state.next()?;
            let mut path = P::default();
            let ret = M::path(keys, &mut path, self.separator);
            return match self.state.handle(ret) {
                None => {
                    continue;
                }
                Some(Ok(_depth)) => Some(Ok(path)),
                Some(Err((_depth, err))) => Some(Err(err)),
            };
        }
    }
}

impl<'a, M, const Y: usize, P, const D: usize> core::iter::FusedIterator
    for PathIter<'a, M, Y, P, D>
where
    M: TreeKey<Y> + ?Sized,
    P: Write + Default + ?Sized,
{
}

/// An iterator over the indices in a `TreeKey`.
///
/// The iterator yields `(indices: [usize; Y], depth: usize)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexIter<M: ?Sized, const Y: usize, const D: usize> {
    state: State<D>,
    _m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize, const D: usize> Default for IndexIter<M, Y, D> {
    fn default() -> Self {
        Self {
            state: State::default(),
            _m: PhantomData,
        }
    }
}

impl<M: TreeKey<Y> + ?Sized, const Y: usize, const D: usize> IndexIter<M, Y, D> {
    /// Limit and start iteration to at and below the provided root key.
    pub fn root<K: IntoKeys>(&mut self, root: K) -> Result<usize, Traversal> {
        let (idx, depth) = M::indices(root)?;
        self.state = State::new(&idx[..depth])?;
        Ok(depth)
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited.
    pub fn count(self) -> Counting<Self> {
        assert!(self.state.depth > D);
        assert!(self.state.root == 0);
        assert!(D >= Y);
        Counting::new(self, M::metadata().count)
    }
}

impl<M, const Y: usize, const D: usize> Iterator for IndexIter<M, Y, D>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = ([usize; D], usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let keys = self.state.next()?;
            let ret = M::traverse_by_key(keys.into_keys(), |_, _, _| Ok(()));
            return match self.state.handle(ret) {
                None => {
                    continue;
                }
                Some(Ok(depth)) => Some((self.state.state, depth)),
                Some(Err((_, ()))) => unreachable!(),
            };
        }
    }
}

impl<M, const Y: usize, const D: usize> FusedIterator for IndexIter<M, Y, D> where
    M: TreeKey<Y> + ?Sized
{
}

/// An iterator over packed indices in a `TreeKey`.
///
/// The iterator yields `Result<(packed: Packed, depth: usize), ()>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedIter<M: ?Sized, const Y: usize, const D: usize> {
    state: State<D>,
    _m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize, const D: usize> Default for PackedIter<M, Y, D> {
    fn default() -> Self {
        Self {
            state: State::default(),
            _m: PhantomData,
        }
    }
}

impl<M: TreeKey<Y> + ?Sized, const Y: usize, const D: usize> PackedIter<M, Y, D> {
    /// Limit and start iteration to at and below the provided root key.
    pub fn root<K: IntoKeys>(&mut self, root: K) -> Result<usize, Traversal> {
        let (idx, depth) = M::indices(root)?;
        self.state = State::new(&idx[..depth])?;
        Ok(depth)
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called or
    /// if the iteration depth has been limited.
    pub fn count(self) -> Counting<Self> {
        assert!(self.state.depth > D);
        assert!(self.state.root == 0);
        assert!(D >= Y);
        Counting::new(self, M::metadata().count)
    }
}

impl<M, const Y: usize, const D: usize> Iterator for PackedIter<M, Y, D>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = Result<Packed, usize>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let keys = self.state.next()?;
            let mut packed = Packed::default();
            let ret = M::packed(keys).map(|(p, depth)| {
                packed = p;
                depth
            });
            return match self.state.handle(ret) {
                None => {
                    continue;
                }
                Some(Ok(_depth)) => Some(Ok(packed)),
                Some(Err((depth, ()))) => Some(Err(depth)),
            };
        }
    }
}

impl<M, const Y: usize, const D: usize> FusedIterator for PackedIter<M, Y, D> where
    M: TreeKey<Y> + ?Sized
{
}
