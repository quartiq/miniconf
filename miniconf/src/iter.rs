use crate::{Error, Packed, Traversal, TreeKey};
use core::{fmt::Write, iter::FusedIterator, marker::PhantomData};

/// Counting wrapper for iterators
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counting<T> {
    iter: T,
    count: usize,
}

impl<T> Counting<T> {
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
            self.count -= 1;
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

/// A managed indices state for iteration of nodes in a `TreeKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct State<const Y: usize> {
    state: [usize; Y],
    depth: usize,
}

impl<const Y: usize> Default for State<Y> {
    fn default() -> Self {
        Self {
            state: [0; Y],
            depth: usize::MAX,
        }
    }
}

impl<const Y: usize> State<Y> {
    /// Try to prepare for the next iteratiion
    ///
    /// Increment current current index and return indices iterator.
    fn next(&mut self) -> Option<impl Iterator<Item = usize> + '_> {
        if self.depth == 0 {
            // Found root leaf (Option/newtype) or done at root
            return None;
        }
        if self.depth != usize::MAX {
            // Not initial state: increment
            self.state[self.depth - 1] += 1;
        }
        Some(self.state.iter().copied())
    }

    /// Handle the result of a `traverse_by_key()` and update `depth` for next iteration.
    fn handle<E>(&mut self, ret: Result<usize, Error<E>>) -> Option<Result<usize, (usize, E)>> {
        match ret {
            // Node found, save depth for increment at next iteration
            Ok(depth) => {
                self.depth = depth;
                Some(Ok(depth))
            }
            // Node not found at finite depth: reset current index, then retry
            Err(Error::Traversal(Traversal::NotFound(depth @ 1..))) => {
                self.state[depth - 1] = 0;
                self.depth = depth - 1;
                None
            }
            Err(Error::Inner(depth, err)) => Some(Err((depth, err))),
            // Traversal::NotFound(0): Not having consumed any name/index, the only possible case
            // is a root leaf (e.g. `Option` or newtype), those however can not return
            // `NotFound` as they don't do key lookup.
            // Traversal::TooShort(_): Excluded by construction (`state.len() == Y` and `Y` being an
            // upper bound to key length as per the `TreeKey<Y>` contract.
            // TooLong, Absent, Finalization, InvalidLead, InvalidInternal:
            // Are not returned by traverse_by_key()
            _ => unreachable!(),
        }
    }
}

/// An iterator over the paths in a `TreeKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathIter<'a, M: ?Sized, const Y: usize, P> {
    state: State<Y>,
    separator: &'a str,
    pm: PhantomData<(P, M)>,
}

impl<'a, M: TreeKey<Y> + ?Sized, const Y: usize, P> PathIter<'a, M, Y, P> {
    pub(crate) fn new(separator: &'a str) -> Self {
        Self {
            state: State::default(),
            separator,
            pm: PhantomData,
        }
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called.
    pub fn count(self) -> Counting<Self> {
        assert!(self.state.depth == usize::MAX);
        Counting::new(self, M::metadata().count)
    }
}

impl<'a, M, const Y: usize, P> Iterator for PathIter<'a, M, Y, P>
where
    M: TreeKey<Y> + ?Sized,
    P: Write + Default,
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

impl<'a, M, const Y: usize, P> core::iter::FusedIterator for PathIter<'a, M, Y, P>
where
    M: TreeKey<Y> + ?Sized,
    P: Write + Default,
{
}

/// An iterator over the indices in a `TreeKey`.
///
/// The iterator yields `(indices: [usize; Y], depth: usize)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexIter<M: ?Sized, const Y: usize> {
    state: State<Y>,
    m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize> Default for IndexIter<M, Y> {
    fn default() -> Self {
        Self {
            state: State::default(),
            m: PhantomData,
        }
    }
}

impl<M: TreeKey<Y> + ?Sized, const Y: usize> IndexIter<M, Y> {
    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called.
    pub fn count(self) -> Counting<Self> {
        assert!(self.state.depth == usize::MAX);
        Counting::new(self, M::metadata().count)
    }
}

impl<M, const Y: usize> Iterator for IndexIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = ([usize; Y], usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let keys = self.state.next()?;
            let ret = M::traverse_by_key(keys, |_, _, _| Ok(()));
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

impl<M, const Y: usize> FusedIterator for IndexIter<M, Y> where M: TreeKey<Y> + ?Sized {}

/// An iterator over packed indices in a `TreeKey`.
///
/// The iterator yields `Result<(packed: Packed, depth: usize), ()>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedIter<M: ?Sized, const Y: usize> {
    state: State<Y>,
    m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize> Default for PackedIter<M, Y> {
    fn default() -> Self {
        Self {
            state: State::default(),
            m: PhantomData,
        }
    }
}

impl<M: TreeKey<Y> + ?Sized, const Y: usize> PackedIter<M, Y> {
    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called.
    pub fn count(self) -> Counting<Self> {
        assert!(self.state.depth == usize::MAX);
        Counting::new(self, M::metadata().count)
    }
}

impl<M, const Y: usize> Iterator for PackedIter<M, Y>
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

impl<M, const Y: usize> FusedIterator for PackedIter<M, Y> where M: TreeKey<Y> + ?Sized {}
