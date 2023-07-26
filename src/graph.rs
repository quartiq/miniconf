pub trait Graph {
    /// Write the `name` of the item specified by `index`.
    /// May not exhaust the iterator if a Leaf is found early. I.e. the index may be too long.
    /// If `Self` is a leaf, nothing will be consumed from `index` or
    /// written to `name` and `Leaf(0)` will be returned.
    /// If `Self` is non-leaf and  `index` is exhausted, nothing will be written to `name` and
    /// `Internal(0)` will be returned.
    /// If `full`, all path elements are written, otherwise only the final element.
    /// Each element written will always be prefixed by the separator.
    fn path<I, N>(
        indices: &mut I,
        path: &mut N,
        sep: &str,
        each: bool,
    ) -> GraphResult<core::fmt::Error>
    where
        I: Iterator<Item = usize>,
        N: core::fmt::Write,
    {
        Self::traverse_by_index(
            indices,
            |_index, name| {
                path.write_str(sep).and_then(|_| path.write_str(name))?;
                Ok(())
            },
            each,
        )
    }
    /// Determine the `index` of the item specified by `path`.
    /// May not exhaust the iterator if leaf is found early. I.e. the path may be too long.
    /// If `Self` is a leaf, nothing will be consumed from `path` or
    /// written to `index` and `Leaf(0)` will be returned.
    /// If `Self` is non-leaf and  `path` is exhausted, nothing will be written to `index` and
    /// `Internal(0)` will be returned.
    /// Entries in `index` at and beyond the `depth` returned are unaffected.
    fn indices<'a, P>(path: &mut P, indices: &mut [usize]) -> GraphResult<SliceShort>
    where
        P: Iterator<Item = &'a str>,
    {
        let mut depth = 0;
        Self::traverse_by_name(
            path,
            |index, _name| {
                if indices.len() < depth {
                    Err(SliceShort)
                } else {
                    indices[depth] = index;
                    depth += 1;
                    Ok(())
                }
            },
            true,
        )
    }

    fn traverse_by_name<'a, P, F, E>(names: &mut P, func: F, internal: bool) -> GraphResult<E>
    where
        P: Iterator<Item = &'a str>,
        F: FnMut(usize, &str) -> Result<(), E>;

    fn traverse_by_index<P, F, E>(indices: &mut P, func: F, internal: bool) -> GraphResult<E>
    where
        P: Iterator<Item = usize>,
        F: FnMut(usize, &str) -> Result<(), E>;
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Error<E> {
    /// Index entry too large at depth
    NotFound(usize),
    /// Invalid number (for `name()`)
    Parse(core::num::ParseIntError),
    /// Inner error, e.g.
    /// Formating error (Write::write_str failure, for `name()`)
    /// or
    /// Index too short (for `index()`)
    Inner(E),
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Ok {
    /// Non-leaf at depth
    Internal(usize),
    /// Leaf at depth
    Leaf(usize),
}

pub type GraphResult<E> = core::result::Result<Ok, Error<E>>;

pub struct SliceShort;

pub trait Up {
    fn up(self) -> Self;
}

impl<E> Up for GraphResult<E> {
    fn up(self) -> Self {
        match self {
            Ok(Ok::Internal(i)) => Ok(Ok::Internal(i + 1)),
            Ok(Ok::Leaf(i)) => Ok(Ok::Leaf(i + 1)),
            Err(Error::NotFound(i)) => Err(Error::NotFound(i + 1)),
            e => e,
        }
    }
}
