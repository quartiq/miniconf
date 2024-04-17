use crate::{Error, Packed, TreeKey};
use core::{fmt::Write, marker::PhantomData};

/// An iterator over nodes in a `TreeKey`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Iter<const Y: usize> {
    /// The iteration state.
    ///
    /// It contains the current field/element index at each path hierarchy level
    /// and needs to be at least as large as the maximum path depth.
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
    fn next<F, E>(&mut self, mut func: F) -> State<E>
    where
        F: FnMut(core::iter::Copied<core::slice::Iter<'_, usize>>) -> Result<usize, Error<E>>,
    {
        match func(self.state.iter().copied()) {
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
            Ok(0) => {
                if self.count == Some(0) {
                    State::Done
                } else {
                    debug_assert_eq!(self.count.unwrap_or(1), 1);
                    self.count = Some(0);
                    State::Leaf(0)
                }
            }
            // Non-root leaf: advance index at current depth
            Ok(depth @ 1..) => {
                self.count = self.count.map(|c| c - 1);
                self.state[depth - 1] += 1;
                State::Leaf(depth)
            }
            Err(Error::Inner(e)) => State::Err(e),
            // * NotFound(0) Not having consumed any name/index, the only possible case
            //   is a leaf (e.g. `Option` or newtype), those however can not return `NotFound`.
            // * TooShort is excluded by construction.
            // * No other errors are returned by traverse_by_key()/path()
            _ => unreachable!(),
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
    /// The separator before each name.
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
            return match self
                .iter
                .next(|keys| M::path(keys, &mut path, self.separator))
            {
                State::Retry => {
                    path = P::default();
                    continue;
                }
                State::Leaf(_depth) => Some(Ok(path)),
                State::Done => None,
                State::Err(e @ core::fmt::Error) => Some(Err(e)),
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
pub struct IndexIter<M: ?Sized, const Y: usize>(Iter<Y>, PhantomData<M>);

impl<M, const Y: usize> IndexIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    pub(crate) fn new(count: Option<usize>) -> Self {
        Self(Iter::new(count), PhantomData)
    }
}

impl<M, const Y: usize> Iterator for IndexIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = ([usize; Y], usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            return match self
                .0
                .next(|keys| M::traverse_by_key(keys, |_, _, _| Ok(())))
            {
                State::Retry => {
                    continue;
                }
                State::Leaf(depth) => {
                    let mut idx = self.0.state;
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
        self.0.size_hint()
    }
}

impl<M, const Y: usize> core::iter::FusedIterator for IndexIter<M, Y> where M: TreeKey<Y> {}

/// An iterator over packed indices in a `TreeKey`.
///
/// The iterator yields `Result<(packed: Packed, depth: usize), ()>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedIter<M: ?Sized, const Y: usize>(Iter<Y>, PhantomData<M>);

impl<M, const Y: usize> PackedIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    pub(crate) fn new(count: Option<usize>) -> Self {
        Self(Iter::new(count), PhantomData)
    }
}

impl<M, const Y: usize> Iterator for PackedIter<M, Y>
where
    M: TreeKey<Y> + ?Sized,
{
    type Item = Result<(Packed, usize), ()>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut packed = Packed::default();
        loop {
            let state = self.0.next(|keys| {
                let (p, depth) = M::packed(keys)?;
                packed = p;
                Ok(depth)
            });
            return match state {
                State::Retry => {
                    packed = Packed::default();
                    continue;
                }
                State::Leaf(depth) => Some(Ok((packed, depth))),
                State::Done => None,
                State::Err(()) => Some(Err(())),
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<M, const Y: usize> core::iter::FusedIterator for PackedIter<M, Y> where M: TreeKey<Y> {}
