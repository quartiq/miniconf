use crate::{Error, Key, Metadata, Miniconf};

/// `Miniconf<D>` for `Option`.
///
/// # Design
///
/// An `Option` may be marked with `#[miniconf(defer(D))]`
/// and be `None` at run-time. This makes the corresponding part of the namespace inaccessible
/// at run-time. It will still be iterated over by [`Miniconf::iter_paths()`] but attempts to
/// `serialize_by_key()` or `deserialize_by_key()` them using the [`Miniconf`] API return in [`Error::Absent`].
///
/// This is intended as a mechanism to provide run-time construction of the namespace. In some
/// cases, run-time detection may indicate that some component is not present. In this case,
/// namespaces will not be exposed for it.
///
/// If the depth specified by the `miniconf(defer(D))` attribute exceeds 1,
/// the `Option` can be used to access content within the inner type.
/// If marked with `#[miniconf(defer(-))]`, and `None` at runtime, the value or the entire sub-tree
/// is inaccessible through `Miniconf::{get,set}_by_key`.
/// If there is no `miniconf` attribute on an `Option` field in a `struct or in an array,
/// JSON `null` corresponds to`None` as usual.

macro_rules! depth {
    ($($d:literal)+) => {$(
        impl<T: Miniconf<{$d - 1}>> Miniconf<$d> for Option<T> {
            fn name_to_index(_value: &str) -> core::option::Option<usize> {
                None
            }

            fn serialize_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
            where
                K: Iterator,
                K::Item: Key,
                S: serde::Serializer,
            {
                if let Some(inner) = self {
                    inner.serialize_by_key(keys, ser)
                } else {
                    Err(Error::Absent(0))
                }
            }

            fn deserialize_by_key<'a, K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
            where
                K: Iterator,
                K::Item: Key,
                D: serde::Deserializer<'a>,
            {
                if let Some(inner) = self {
                    inner.deserialize_by_key(keys, de)
                } else {
                    Err(Error::Absent(0))
                }
            }

            fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
            where
                K: Iterator,
                K::Item: Key,
                F: FnMut(usize, &str) -> Result<(), E>,
            {
                T::traverse_by_key(keys, func)
            }

            fn metadata() -> Metadata {
                T::metadata()
            }
        }
    )+}
}

depth!(2 3 4 5 6 7 8);

impl<T: serde::Serialize + serde::de::DeserializeOwned> Miniconf for core::option::Option<T> {
    fn name_to_index(_value: &str) -> core::option::Option<usize> {
        None
    }

    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Iterator,
        S: serde::Serializer,
    {
        if keys.next().is_some() {
            return Err(Error::TooLong(0));
        }

        if let Some(inner) = self {
            serde::Serialize::serialize(inner, ser)?;
            Ok(0)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn deserialize_by_key<'a, K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        D: serde::Deserializer<'a>,
    {
        if keys.next().is_some() {
            return Err(Error::TooLong(0));
        }

        if let Some(inner) = self {
            *inner = serde::Deserialize::deserialize(de)?;
            Ok(0)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn traverse_by_key<K, F, E>(_keys: K, _func: F) -> Result<usize, Error<E>>
    where
        F: FnMut(usize, &str) -> Result<(), E>,
    {
        Ok(0)
    }

    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }
}
