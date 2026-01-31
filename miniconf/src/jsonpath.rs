use core::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::{DescendError, IntoKeys, KeysIter, Schema, Transcode};

/// JSON style path notation iterator
///
/// This is only styled after JSON notation, it does not adhere to it.
/// Supported are both dot and key notation with and without
/// names enclosed by `'` as well as various mixtures:
///
/// ```
/// use miniconf::JsonPathIter;
/// let path = ["foo", "bar", "4", "baz", "5", "6"];
/// for valid in [
///     ".foo.bar[4].baz[5][6]",
///     "['foo']['bar'][4]['baz'][5][6]",
///     ".foo['bar'].4.'baz'['5'].'6'",
/// ] {
///     assert_eq!(&path[..], JsonPathIter::new(valid).collect::<Vec<_>>());
/// }
///
/// for short in ["'", "[", "['"] {
///     assert!(JsonPathIter::new(short).next().is_none());
/// }
/// ```
///
/// # Limitations
///
/// * No attempt at validating conformance
/// * Does not support any escaping
#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize, Hash)]
#[repr(transparent)]
#[serde(transparent)]
pub struct JsonPathIter<'a>(&'a str);

impl<'a> JsonPathIter<'a> {
    /// Interpret a str as a JSON path to be iterated over.
    pub fn new(value: &'a str) -> Self {
        Self(value)
    }
}

impl core::fmt::Display for JsonPathIter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> Iterator for JsonPathIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        enum Close {
            Inclusive(&'static str),
            Exclusive(&'static [char]),
        }
        use Close::*;
        for (open, close) in [
            (".'", Inclusive("'")),
            (".", Exclusive(&['.', '['])),
            ("['", Inclusive("']")),
            ("[", Inclusive("]")),
        ] {
            if let Some(rest) = self.0.strip_prefix(open) {
                let (pre, post) = match close {
                    Exclusive(close) => rest
                        .find(close)
                        .map(|i| rest.split_at(i))
                        .unwrap_or((rest, "")),
                    Inclusive(close) => rest.split_once(close)?,
                };
                self.0 = post;
                return Some(pre);
            }
        }
        None
    }
}

impl core::iter::FusedIterator for JsonPathIter<'_> {}

/// JSON style path notation
///
/// `T` can be `Write` for `Transcode` with the following behavior:
/// * Named fields (struct) are encoded in dot notation.
/// * Indices (tuple struct, array) are encoded in index notation
///
/// `T` can be `AsRef<str>` for `IntoKeys` with the behavior described in [`JsonPathIter`].
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize, Hash,
)]
#[repr(transparent)]
#[serde(transparent)]
pub struct JsonPath<T: ?Sized>(pub T);

impl<T> JsonPath<T> {
    /// Extract the inner value
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T: core::fmt::Display> core::fmt::Display for JsonPath<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a, T: AsRef<str> + ?Sized> IntoKeys for JsonPath<&'a T> {
    type IntoKeys = KeysIter<JsonPathIter<'a>>;
    fn into_keys(self) -> Self::IntoKeys {
        JsonPathIter(self.0.as_ref()).into_keys()
    }
}

impl<'a, T: AsRef<str> + ?Sized> IntoKeys for &'a JsonPath<T> {
    type IntoKeys = <JsonPath<&'a str> as IntoKeys>::IntoKeys;
    fn into_keys(self) -> Self::IntoKeys {
        JsonPathIter(self.0.as_ref()).into_keys()
    }
}

impl<T: Write + ?Sized> Transcode for JsonPath<T> {
    type Error = core::fmt::Error;

    fn transcode(
        &mut self,
        schema: &Schema,
        keys: impl IntoKeys,
    ) -> Result<(), DescendError<Self::Error>> {
        schema.descend(keys.into_keys(), |_meta, idx_internal| {
            if let Some((index, internal)) = idx_internal {
                if let Some(name) = internal.get_name(index) {
                    debug_assert!(!name.contains(['.', '\'', '[', ']']));
                    self.0.write_char('.')?;
                    self.0.write_str(name)?;
                } else {
                    self.0.write_char('[')?;
                    self.0.write_str(itoa::Buffer::new().format(index))?;
                    self.0.write_char(']')?;
                }
            }
            Ok(())
        })
    }
}
