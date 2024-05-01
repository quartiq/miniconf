use serde::{Deserialize, Serialize};

/// JSON style path notation
///
/// This is only styled after JSON notation, it does not adhere to it.
/// Supported are both dot and key notation with and without
/// names enclosed by `'` as well as mixtures:
///
/// ```
/// # use miniconf::JsonPath;
/// let path = ["foo", "bar", "4", "baz", "5", "6"];
/// for valid in [
///     ".foo.bar[4].baz[5][6]",
///     "['foo']['bar'][4]['baz'][5][6]",
///     ".foo['bar'].4.'baz'['5'].'6'",
/// ] {
///     assert_eq!(&path[..], JsonPath::from(valid).collect::<Vec<_>>());
/// }
///
/// for short in ["'", "[", "['"] {
///     assert!(JsonPath::from(short).next().is_none());
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
pub struct JsonPath<'a>(&'a str);

impl<'a, T> From<&'a T> for JsonPath<'a>
where
    T: AsRef<str> + ?Sized,
{
    fn from(value: &'a T) -> Self {
        Self(value.as_ref())
    }
}

impl<'a> From<JsonPath<'a>> for &'a str {
    fn from(value: JsonPath<'a>) -> Self {
        value.0
    }
}

impl<'a> Iterator for JsonPath<'a> {
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
