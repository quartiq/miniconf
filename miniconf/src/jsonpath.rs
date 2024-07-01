use core::{
    fmt::Write,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Serialize};

use crate::{traverse, IntoKeys, KeysIter, Node, Transcode, Traversal, TreeKey};

/// JSON style path notation
///
/// This is only styled after JSON notation, it does not adhere to it.
/// Supported are both dot and key notation with and without
/// names enclosed by `'` as well as mixtures:
///
/// ```
/// use miniconf::JsonPathIter;
/// let path = ["foo", "bar", "4", "baz", "5", "6"];
/// for valid in [
///     ".foo.bar[4].baz[5][6]",
///     "['foo']['bar'][4]['baz'][5][6]",
///     ".foo['bar'].4.'baz'['5'].'6'",
/// ] {
///     assert_eq!(&path[..], JsonPathIter::from(valid).collect::<Vec<_>>());
/// }
///
/// for short in ["'", "[", "['"] {
///     assert!(JsonPathIter::from(short).next().is_none());
/// }
/// ```
///
/// # Limitations
///
/// * No attempt at validating conformance.
/// * It does not support any escaping.
///
#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct JsonPathIter<'a>(&'a str);

impl<'a, T> From<&'a T> for JsonPathIter<'a>
where
    T: AsRef<str> + ?Sized,
{
    fn from(value: &'a T) -> Self {
        Self(value.as_ref())
    }
}

impl<'a> From<JsonPathIter<'a>> for &'a str {
    fn from(value: JsonPathIter<'a>) -> Self {
        value.0
    }
}

impl<'a> Iterator for JsonPathIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        // Reappropriation of `Result` as `Either`
        for (open, close) in [
            (".'", Ok("'")),             // "'" inclusive
            (".", Err(&['.', '['][..])), // '.' or '[' exclusive
            ("['", Ok("']")),            // "']" inclusive
            ("[", Ok("]")),              // "]" inclusive
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

/// Wrapper to transcode into a normalized JSON path
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize, Hash,
)]
#[repr(transparent)]
#[serde(transparent)]
pub struct JsonPath<T>(pub T);

impl<T> From<T> for JsonPath<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> JsonPath<T> {
    /// Extract the inner value
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> Deref for JsonPath<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for JsonPath<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'a, T: AsRef<str>> IntoKeys for &'a JsonPath<T> {
    type IntoKeys = KeysIter<JsonPathIter<'a>>;
    fn into_keys(self) -> Self::IntoKeys {
        JsonPathIter::from(self.0.as_ref()).into_keys()
    }
}

impl<T: Write> Transcode for JsonPath<T> {
    fn transcode<M, const Y: usize, K>(&mut self, keys: K) -> Result<Node, Traversal>
    where
        Self: Sized,
        M: TreeKey<Y> + ?Sized,
        K: IntoKeys,
    {
        traverse(M::traverse_by_key(
            keys.into_keys(),
            |index, name, _len| match name {
                Some(name) => {
                    self.0.write_char('.').or(Err(()))?;
                    self.0.write_str(name).or(Err(()))
                }
                None => {
                    self.0.write_char('[').or(Err(()))?;
                    self.0
                        .write_str(itoa::Buffer::new().format(index))
                        .or(Err(()))?;
                    self.0.write_char(']').or(Err(()))
                }
            },
        ))
    }
}
