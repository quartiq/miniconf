use crate::{Error, Packed, TreeKey};
use core::{fmt::Write, marker::PhantomData};

// core::iter::ExactSizeIterator would be applicable if `count.is_some()`.`

/// An iterator over nodes in a `TreeKey`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Iter<const Y: usize> {
    /// The iteration state.
    ///
    /// It contains the current field/element index at each path hierarchy level
    /// and needs to be at least as large as the maximum path depth (ensured by
    /// `TreeKey<Y>` contract).
    state: [usize; Y],

    /// The remaining length of the iterator.
    ///
    /// It is used to provide an exact and trusted [Iterator::size_hint] ([core::iter::TrustedLen]).
    ///
    /// It may be None to indicate unknown length.
    count: Option<usize>,
}

impl<const Y: usize> Iter<Y> {
    fn new(count: Option<usize>) -> Self {
        Self {
            count,
            state: [0; Y],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
enum State<E> {
    Leaf(usize),
    Done,
    Retry,
    Err(E),
}

impl<const Y: usize> Iter<Y> {
    fn handle<E>(&mut self, ret: Result<usize, Error<E>>) -> State<E> {
        match ret {
            // Out of valid indices at the root: iteration done
            Err(Error::NotFound(1)) => {
                debug_assert_eq!(self.count.unwrap_or_default(), 0);
                State::Done
            }
            // Node not found at depth: reset current index, increment parent index,
            // then retry
            Err(Error::NotFound(depth @ 2..)) => {
                self.state[depth - 1] = 0;
                self.state[depth - 2] += 1;
                State::Retry
            }
            // Found a leaf at the root: leaf Option/newtype
            // Since there is no way to end iteration by hoping for `NotFound` on a leaf Option,
            // we force the count to Some(0) and trigger on that.
            Ok(0) if matches!(self.count, Some(0)) => State::Done,
            Ok(0) => {
                debug_assert_eq!(self.count.unwrap_or(1), 1);
                self.count = Some(0);
                State::Leaf(0)
            }
            // Non-root leaf (depth @ 1..): advance index at current depth
            Ok(depth) => {
                self.count = self.count.map(|c| c - 1);
                self.state[depth - 1] += 1;
                State::Leaf(depth)
            }
            Err(Error::Inner(e)) => State::Err(e),
            // NotFound(0): Not having consumed any name/index, the only possible case
            // is a root leaf (e.g. `Option` or newtype), those however can not return
            // `NotFound` as they don't do key lookup.
            Err(Error::NotFound(0)) |
            // TooShort: Excluded by construction (`state.len() == Y` and `Y` being an
            // upper bound to key length as per the `TreeKey<Y>` contract.
            Err(Error::TooShort(_)) |
            // TooLong, Absent, Finalization, InvalidLead, InvalidInternal:
            // Are not returned by traverse_by_key()
            Err(Error::TooLong(_)) |
            Err(Error::Absent(_)) |
            Err(Error::Finalization(_)) |
            Err(Error::InvalidInternal(_, _)) |
            Err(Error::InvalidLeaf(_, _))
            => unreachable!(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}

/// An iterator over the paths in a `TreeKey`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathIter<'a, M: ?Sized, const Y: usize, P> {
    iter: Iter<Y>,
    pm: PhantomData<(P, M)>,
    separator: &'a str,
}

impl<'a, M, const Y: usize, P> PathIter<'a, M, Y, P>
where
    M: TreeKey<Y> + ?Sized,
{
    pub(crate) fn new(separator: &'a str, count: Option<usize>) -> Self {
        Self {
            iter: Iter::new(count),
            pm: PhantomData,
            separator,
        }
    }
}

impl<'a, M, const Y: usize, P> Iterator for PathIter<'a, M, Y, P>
where
    M: TreeKey<Y> + ?Sized,
    P: Write + Default,
{
    type Item = Result<P, core::fmt::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = P::default();

        loop {
            return match self.iter.handle(M::path(
                self.iter.state.iter().copied(),
                &mut path,
                self.separator,
            )) {
                State::Retry => {
                    path = P::default();
                    continue;
                }
                State::Leaf(_depth) => Some(Ok(path)),
                State::Done => None,
                State::Err(e) => Some(Err(e)),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexIter<M: ?Sized, const Y: usize> {
    iter: Iter<Y>,
    m: PhantomData<M>,
}

impl<M, const Y: usize> IndexIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    pub(crate) fn new(count: Option<usize>) -> Self {
        Self {
            iter: Iter::new(count),
            m: PhantomData,
        }
    }
}

impl<M, const Y: usize> Iterator for IndexIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = ([usize; Y], usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            return match self.iter.handle(M::traverse_by_key(
                self.iter.state.iter().copied(),
                |_, _, _| Ok(()),
            )) {
                State::Retry => {
                    continue;
                }
                State::Leaf(depth) => {
                    let mut idx = self.iter.state;
                    if depth > 0 {
                        // Undo the index advancement in Iter::next()
                        idx[depth - 1] -= 1;
                    }
                    Some((idx, depth))
                }
                State::Done => None,
                State::Err(()) => unreachable!(),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<M, const Y: usize> core::iter::FusedIterator for IndexIter<M, Y> where M: TreeKey<Y> {}

/// An iterator over packed indices in a `TreeKey`.
///
/// The iterator yields `Result<(packed: Packed, depth: usize), ()>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedIter<M: ?Sized, const Y: usize> {
    iter: Iter<Y>,
    m: PhantomData<M>,
}

impl<M, const Y: usize> PackedIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    pub(crate) fn new(count: Option<usize>) -> Self {
        Self {
            iter: Iter::new(count),
            m: PhantomData,
        }
    }
}

impl<M, const Y: usize> Iterator for PackedIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = Result<Packed, ()>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let mut packed = Packed::default();
            let ret = M::packed(self.iter.state.iter().copied()).map(|(p, depth)| {
                packed = p;
                depth
            });
            return match self.iter.handle(ret) {
                State::Retry => {
                    continue;
                }
                State::Leaf(_depth) => Some(Ok(packed)),
                State::Done => None,
                State::Err(()) => Some(Err(())),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<M, const Y: usize> core::iter::FusedIterator for PackedIter<M, Y> where M: TreeKey<Y> {}
