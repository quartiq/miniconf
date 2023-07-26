use crate::{graph, IterError, Metadata, SerDe};
use core::{fmt::Write, marker::PhantomData};

/// An iterator over the paths in a Miniconf namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiniconfIter<M, S, const L: usize, P> {
    /// Zero-size marker field to allow being generic over M and gaining access to M.
    marker: PhantomData<(M, S, P)>,

    /// The iteration state.
    ///
    /// It contains the current field/element index at each path hierarchy level
    /// and needs to be at least as large as the maximum path depth.
    state: [usize; L],

    /// The remaining length of the iterator.
    ///
    /// It is used to provide an exact and trusted [Iterator::size_hint].
    /// C.f. [core::iter::TrustedLen].
    ///
    /// It may be None to indicate unknown length.
    count: Option<usize>,
}

impl<M, S, const L: usize, P> Default for MiniconfIter<M, S, L, P> {
    fn default() -> Self {
        Self {
            count: None,
            marker: PhantomData,
            state: [0; L],
        }
    }
}

impl<M: SerDe<S>, S, const L: usize, P> MiniconfIter<M, S, L, P> {
    pub fn metadata() -> Result<Metadata, IterError> {
        let meta = M::metadata(M::SEPARATOR.len_utf8());
        if L < meta.max_depth {
            return Err(IterError::Depth);
        }
        Ok(meta)
    }

    pub fn new() -> Result<Self, IterError> {
        let meta = Self::metadata()?;
        Ok(Self {
            count: Some(meta.count),
            ..Default::default()
        })
    }
}

impl<M: SerDe<S> + graph::Graph, S, const L: usize, P: Write + Default> Iterator
    for MiniconfIter<M, S, L, P>
{
    type Item = P;

    fn next(&mut self) -> Option<Self::Item> {
        let mut path = Self::Item::default();

        loop {
            return match M::name(
                &mut self.state.iter().copied(),
                &mut path,
                M::SEPARATOR.encode_utf8(&mut [0; 4]),
                true,
            ) {
                Err(graph::Error::NotFound(0)) => {
                    debug_assert_eq!(self.count.unwrap_or_default(), 0);
                    None
                }
                Ok(graph::Ok::Leaf(0)) if self.count == Some(0) => None,
                Ok(graph::Ok::Leaf(0)) => {
                    debug_assert_eq!(self.count.unwrap_or(1), 1);
                    self.count = Some(0);
                    Some(path)
                }
                Ok(graph::Ok::Leaf(depth)) => {
                    self.count = self.count.map(|c| c - 1);
                    self.state[depth - 1] += 1;
                    Some(path)
                }
                Err(graph::Error::NotFound(depth)) => {
                    path = Self::Item::default();
                    self.state[depth - 1] += 1;
                    self.state[depth] = 0;
                    continue;
                }
                Ok(graph::Ok::Internal(_)) => {
                    panic!("state too short");
                }
                e => {
                    e.unwrap();
                    None
                }
            };
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.count.unwrap_or_default(), self.count)
    }
}
