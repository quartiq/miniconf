use crate::{Error, Packed, TreeKey};
use core::{fmt::Write, iter::FusedIterator, marker::PhantomData};

/// Counting wrapper for iterators
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counting<T> {
    count: usize,
    iter: T,
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
            debug_assert_eq!(self.count, 0);
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count, Some(self.count))
    }
}

impl<T: Iterator> ExactSizeIterator for Counting<T> {}
impl<T: FusedIterator> FusedIterator for Counting<T> {}
// unsafe impl<T: Iterator> core::iter::TrustedLen for Counting<T> {}

/// An iterator over nodes in a `TreeKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Iter<const Y: usize> {
    state: [usize; Y],
    depth: Option<usize>,
}

impl<const Y: usize> Default for Iter<Y> {
    fn default() -> Self {
        Self {
            state: [0; Y],
            depth: None,
        }
    }
}

impl<const Y: usize> Iter<Y> {
    fn next(&mut self) -> Option<impl Iterator<Item = usize> + '_> {
        match self.depth {
            // Initial state, `handle()` will set a depth
            None => Some(self.state.iter().copied()),
            // Found root leaf (Option/newtype) or done at root
            Some(0) => None,
            // Increment current depth
            Some(depth) => {
                self.state[depth - 1] += 1;
                Some(self.state.iter().copied())
            }
        }
    }

    fn handle<E>(&mut self, ret: Result<usize, Error<E>>) -> Option<Result<usize, (usize, E)>> {
        match ret {
            // Node found
            Ok(depth) => {
                self.depth = Some(depth);
                Some(Ok(depth))
            }
            // Node not found at finite depth: reset current index, then retry
            Err(Error::NotFound(depth @ 1..)) => {
                self.state[depth - 1] = 0;
                self.depth = Some(depth - 1);
                None
            }
            Err(Error::Inner(depth, err)) => Some(Err((depth, err))),
            // NotFound(0): Not having consumed any name/index, the only possible case
            // is a root leaf (e.g. `Option` or newtype), those however can not return
            // `NotFound` as they don't do key lookup.
            // We write NotFound(_) as e.g. rust 1.70.0 isn't smart enough to prove coverage.
            Err(Error::NotFound(_)) |
            // TooShort: Excluded by construction (`state.len() == Y` and `Y` being an
            // upper bound to key length as per the `TreeKey<Y>` contract.
            Err(Error::TooShort(_)) |
            // TooLong, Absent, Finalization, InvalidLead, InvalidInternal:
            // Are not returned by traverse_by_key()
            Err(Error::TooLong(_)) |
            Err(Error::Absent(_)) |
            Err(Error::Finalization(_)) |
            Err(Error::Access(_, _)) |
            Err(Error::Invalid(_, _))
            => unreachable!(),
        }
    }
}

/// An iterator over the paths in a `TreeKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathIter<'a, M: ?Sized, const Y: usize, P> {
    iter: Iter<Y>,
    separator: &'a str,
    pm: PhantomData<(P, M)>,
}

impl<'a, M: ?Sized + TreeKey<Y>, const Y: usize, P> PathIter<'a, M, Y, P> {
    pub(crate) fn new(separator: &'a str) -> Self {
        Self {
            iter: Iter::default(),
            separator,
            pm: PhantomData,
        }
    }

    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called.
    pub fn count(self) -> Counting<Self> {
        assert!(self.iter.depth.is_none());
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
            let keys = self.iter.next()?;
            let mut path = P::default();
            let ret = M::path(keys, &mut path, self.separator);
            return match self.iter.handle(ret) {
                None => {
                    continue;
                }
                Some(Ok(_depth)) => Some(Ok(path)),
                Some(Err((_depth, e))) => Some(Err(e)),
            };
        }
    }
}

impl<'a, M, const Y: usize, P> core::iter::FusedIterator for PathIter<'a, M, Y, P>
where
    M: TreeKey<Y>,
    P: Write + Default,
{
}

/// An iterator over the indices in a `TreeKey`.
///
/// The iterator yields `(indices: [usize; Y], depth: usize)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexIter<M: ?Sized, const Y: usize> {
    iter: Iter<Y>,
    m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize> Default for IndexIter<M, Y> {
    fn default() -> Self {
        Self {
            iter: Iter::default(),
            m: PhantomData,
        }
    }
}

impl<M: ?Sized + TreeKey<Y>, const Y: usize> IndexIter<M, Y> {
    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called.
    pub fn count(self) -> Counting<Self> {
        assert!(self.iter.depth.is_none());
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
            let keys = self.iter.next()?;
            let ret = M::traverse_by_key(keys, |_, _, _| Ok(()));
            return match self.iter.handle(ret) {
                None => {
                    continue;
                }
                Some(Ok(depth)) => Some((self.iter.state, depth)),
                Some(Err((_, ()))) => unreachable!(),
            };
        }
    }
}

impl<M, const Y: usize> FusedIterator for IndexIter<M, Y> where M: TreeKey<Y> {}

/// An iterator over packed indices in a `TreeKey`.
///
/// The iterator yields `Result<(packed: Packed, depth: usize), ()>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedIter<M: ?Sized, const Y: usize> {
    iter: Iter<Y>,
    m: PhantomData<M>,
}

impl<M: ?Sized, const Y: usize> Default for PackedIter<M, Y> {
    fn default() -> Self {
        Self {
            iter: Iter::default(),
            m: PhantomData,
        }
    }
}

impl<M: ?Sized + TreeKey<Y>, const Y: usize> PackedIter<M, Y> {
    /// Wrap the iterator in an exact size counting iterator.
    ///
    /// Note(panic): Panics, if the iterator had `next()` called.
    pub fn count(self) -> Counting<Self> {
        assert!(self.iter.depth.is_none());
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
            let keys = self.iter.next()?;
            let mut packed = Packed::default();
            let ret = M::packed(keys).map(|(p, depth)| {
                packed = p;
                depth
            });
            return match self.iter.handle(ret) {
                None => {
                    continue;
                }
                Some(Ok(_depth)) => Some(Ok(packed)),
                Some(Err((depth, ()))) => Some(Err(depth)),
            };
        }
    }
}

impl<M, const Y: usize> FusedIterator for PackedIter<M, Y> where M: TreeKey<Y> {}
