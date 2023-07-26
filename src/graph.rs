pub trait Graph {
    /// Write the `name` of the item specified by `index`.
    /// May not exhaust the iterator if a Leaf is found early. I.e. the index may be too long.
    /// If `Self` is a leaf, nothing will be consumed from `index` or
    /// written to `name` and `Leaf(0)` will be returned.
    /// If `Self` is non-leaf and  `index` is exhausted, nothing will be written to `name` and
    /// `Internal(0)` will be returned.
    /// If `full`, all path elements are written, otherwise only the final element.
    /// Each element written will always be prefixed by the separator.
    fn name<I: Iterator<Item = usize>, N: core::fmt::Write>(
        index: &mut I,
        name: &mut N,
        separator: &str,
        full: bool,
    ) -> Result;
    /// Determine the `index` of the item specified by `path`.
    /// May not exhaust the iterator if leaf is found early. I.e. the path may be too long.
    /// If `Self` is a leaf, nothing will be consumed from `path` or
    /// written to `index` and `Leaf(0)` will be returned.
    /// If `Self` is non-leaf and  `path` is exhausted, nothing will be written to `index` and
    /// `Internal(0)` will be returned.
    /// Entries in `index` at and beyond the `depth` returned are unaffected.
    fn index<'a, P: Iterator<Item = &'a str>>(path: &mut P, index: &mut [usize]) -> Result;
}

#[non_exhaustive]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Error {
    /// Index entry too large at depth
    NotFound(usize),
    /// Index too short (for `index()`)
    TooShort,
    /// Formating error (Write::write_str failute)
    Fmt(core::fmt::Error),
    /// Invalid number (for `name()`)
    Parse(core::num::ParseIntError),
}

#[non_exhaustive]
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Ok {
    /// Non-leaf at depth
    Internal(usize),
    /// Leaf at depth
    Leaf(usize),
}

pub type Result = core::result::Result<Ok, Error>;

impl From<core::num::ParseIntError> for Error {
    fn from(value: core::num::ParseIntError) -> Self {
        Self::Parse(value)
    }
}

impl From<core::fmt::Error> for Error {
    fn from(value: core::fmt::Error) -> Self {
        Self::Fmt(value)
    }
}

pub trait Up {
    fn up(self) -> Self;
}

impl Up for Result {
    fn up(self) -> Self {
        match self {
            Ok(Ok::Internal(i)) => Ok(Ok::Internal(i + 1)),
            Ok(Ok::Leaf(i)) => Ok(Ok::Leaf(i + 1)),
            Err(Error::NotFound(i)) => Err(Error::NotFound(i + 1)),
            e => e,
        }
    }
}
