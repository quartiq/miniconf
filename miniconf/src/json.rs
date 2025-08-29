//! `TreeSerialize`/`TreeDeserialize` with "JSON and `/`".
//!
//! Access items with `'/'` as path separator and JSON (from `serde-json-core`)
//! as serialization/deserialization payload format.
//!
//! Paths used here are reciprocal to `TreeKey::lookup::<Path<_, '/'>, _>(...)`/
//! `TreeKey::nodes::<Path<_, '/'>>()`.
//!
//! ```
//! use miniconf::{json, Leaf, Tree};
//! #[derive(Tree, Default)]
//! struct S {
//!     foo: Leaf<u32>,
//!     bar: [Leaf<u16>; 2],
//! };
//! let mut s = S::default();
//! json::set(&mut s, "/bar/1", b"9").unwrap();
//! assert_eq!(*s.bar[1], 9);
//! let mut buf = [0u8; 10];
//! let len = json::get(&mut s, "/bar/1", &mut buf[..]).unwrap();
//! assert_eq!(&buf[..len], b"9");
//! ```

use serde_json_core::{de, ser};

use crate::{IntoKeys, Path, SerDeError, TreeDeserialize, TreeSerialize};

/// Update a node by path.
///
/// # Args
/// * `tree` - The `TreeDeserialize` to operate on.
/// * `path` - The path to the node. Everything before the first `'/'` is ignored.
/// * `data` - The serialized data making up the content.
///
/// # Returns
/// The number of bytes consumed from `data` or an [Error].
#[inline]
pub fn set<'de>(
    tree: &mut (impl TreeDeserialize<'de> + ?Sized),
    path: &str,
    data: &'de [u8],
) -> Result<usize, SerDeError<de::Error>> {
    set_by_key(tree, Path::<_, '/'>::from(path), data)
}

/// Retrieve a serialized value by path.
///
/// # Args
/// * `tree` - The `TreeDeserialize` to operate on.
/// * `path` - The path to the node. Everything before the first `'/'` is ignored.
/// * `data` - The buffer to serialize the data into.
///
/// # Returns
/// The number of bytes used in the `data` buffer or an [Error].
#[inline]
pub fn get(
    tree: &(impl TreeSerialize + ?Sized),
    path: &str,
    data: &mut [u8],
) -> Result<usize, SerDeError<ser::Error>> {
    get_by_key(tree, Path::<_, '/'>::from(path), data)
}

/// Update a node by key.
///
/// # Returns
/// The number of bytes consumed from `data` or an [Error].
#[inline]
pub fn set_by_key<'de>(
    tree: &mut (impl TreeDeserialize<'de> + ?Sized),
    keys: impl IntoKeys,
    data: &'de [u8],
) -> Result<usize, SerDeError<de::Error>> {
    let mut de = de::Deserializer::new(data, None);
    tree.deserialize_by_key(keys.into_keys(), &mut de)?;
    de.end().map_err(SerDeError::Finalization)
}

/// Retrieve a serialized value by key.
///
/// # Returns
/// The number of bytes used in the `data` buffer or an [Error].
#[inline]
pub fn get_by_key(
    tree: &(impl TreeSerialize + ?Sized),
    keys: impl IntoKeys,
    data: &mut [u8],
) -> Result<usize, SerDeError<ser::Error>> {
    let mut ser = ser::Serializer::new(data);
    tree.serialize_by_key(keys.into_keys(), &mut ser)?;
    Ok(ser.end())
}
