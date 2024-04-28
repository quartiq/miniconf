use crate::{Traversal, TreeKey};

/// Capability to convert a key into a node index for a given `M: TreeKey`
pub trait Key {
    /// Convert the key `self` to a `usize` index
    fn find<const Y: usize, M: TreeKey<Y>>(&self) -> Option<usize>;
}

// `usize` index as Key
impl Key for usize {
    fn find<const Y: usize, M>(&self) -> Option<usize> {
        Some(*self)
    }
}

// &str name as Key
impl Key for &str {
    fn find<const Y: usize, M: TreeKey<Y>>(&self) -> Option<usize> {
        M::name_to_index(self)
    }
}

/// Capability to yield [`Key`]s
pub trait Keys {
    /// Look up the next key in a [`TreeKey`] and convert to `usize` index.
    fn next<const Y: usize, M: TreeKey<Y>>(&mut self) -> Result<usize, Traversal>;

    /// Return whether there are more keys.
    ///
    /// This may mutate and consume remaining keys.
    fn is_empty(&mut self) -> bool;
}

impl<T> Keys for T
where
    T: Iterator,
    T::Item: Key,
{
    fn next<const Y: usize, M: TreeKey<Y>>(&mut self) -> Result<usize, Traversal> {
        let index = Iterator::next(self).ok_or(Traversal::TooShort(0))?;
        index.find::<Y, M>().ok_or(Traversal::NotFound(1))
    }

    fn is_empty(&mut self) -> bool {
        self.next().is_none()
    }
}

/// Capability to be converted into a [`Keys`]
pub trait IntoKeys {
    /// The specific [`Keys`] implementor.
    type IntoKeys: Keys;

    /// Convert `self` into a [`Keys`] implementor.
    fn into_keys(self) -> Self::IntoKeys;
}

impl<T> IntoKeys for T
where
    T: IntoIterator,
    T::IntoIter: Keys,
{
    type IntoKeys = T::IntoIter;

    fn into_keys(self) -> Self::IntoKeys {
        self.into_iter()
    }
}

/// JSON style path notation
///
/// Supported are both dot an key notation
/// as well as mixtures:
///
/// ```
/// # #[cfg(feature = "std")]
/// # {
/// # use miniconf::JsonPath;
/// let path = ["foo", "bar", "4", "baz", "5", "6"];
/// for valid in [
///     ".foo.bar[4].baz[5][6]",
///     "['foo']['bar'][4]['baz'][5][6]",
///     ".foo['bar'].4.baz['5'][6]",
/// ] {
///     assert_eq!(&path[..], JsonPath::new(valid).collect::<Vec<_>>());
/// }
///
/// for short in ["'", "[", ""] {
///     assert!(JsonPath::new(short).next().is_none());
/// }
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct JsonPath<'a>(&'a str);

impl<'a> JsonPath<'a> {
    /// Create a new `JsonPath`
    pub fn new(path: &'a str) -> Self {
        Self(path)
    }
}

impl<'a> Iterator for JsonPath<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        for (open, close) in [
            (".", Err(&['.', '['][..])),
            ("['", Ok("']")),
            ("[", Ok("]")),
        ] {
            if let Some(rest) = self.0.strip_prefix(open) {
                let (end, sep) = match close {
                    Err(close) => (rest.find(close).unwrap_or(rest.len()), 0),
                    Ok(close) => (rest.find(close)?, close.len()),
                };
                let (next, rest) = rest.split_at(end);
                self.0 = &rest[sep..];
                return Some(next);
            }
        }
        None
    }
}
