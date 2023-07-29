use crate::{Error, Key, Metadata, Miniconf};
use core::ops::{Deref, DerefMut};

/// An `Option` that exposes its value through their [`Miniconf`] implementation.
///
/// # Design
///
/// Miniconf supports optional values in two forms.
///
/// In both forms, the `Option` may be marked with `#[miniconf(defer)]`
/// and be `None` at run-time. This makes the corresponding part of the namespace inaccessible
/// at run-time. It will still be iterated over by [`Miniconf::iter_paths()`] but attempts to
/// `get_by_key()` or `set_by_key()` them using the [`Miniconf`] API return in [`Error::Absent`].
///
/// This is intended as a mechanism to provide run-time construction of the namespace. In some
/// cases, run-time detection may indicate that some component is not present. In this case,
/// namespaces will not be exposed for it.
///
/// The first form is the [`miniconf::Option`](Option) type which optionally exposes its
/// interior `Miniconf` value as a sub-tree. An [`miniconf::Option`](Option) should usually be
/// `#[miniconf(defer)]`.
///
/// Miniconf also allows for the normal usage of Rust [`core::option::Option`] types. In this case,
/// the `Option` can be used to atomically access the content within. If marked with `#[miniconf(defer)]`
/// and `None` at runtime, it is inaccessible through `Miniconf`. Otherwise, JSON `null` corresponds to
/// `None` as usual.
///
/// # Construction
///
/// An `miniconf::Option` can be constructed using [`From<core::option::Option>`]/[`Into<miniconf::Option>`]
/// and the contained value can be accessed through [`Deref`]/[`DerefMut`].
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Debug,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[repr(transparent)]
pub struct Option<T>(core::option::Option<T>);

impl<T> Deref for Option<T> {
    type Target = core::option::Option<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Option<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> AsRef<core::option::Option<T>> for Option<T> {
    fn as_ref(&self) -> &core::option::Option<T> {
        self
    }
}

impl<T> AsMut<core::option::Option<T>> for Option<T> {
    fn as_mut(&mut self) -> &mut core::option::Option<T> {
        self
    }
}

impl<T> From<core::option::Option<T>> for Option<T> {
    fn from(x: core::option::Option<T>) -> Self {
        Self(x)
    }
}

impl<T> From<Option<T>> for core::option::Option<T> {
    fn from(x: Option<T>) -> Self {
        x.0
    }
}

// This overrides the impl on core::option::Option through Deref
impl<T: Miniconf> Miniconf for Option<T> {
    fn name_to_index(_value: &str) -> core::option::Option<usize> {
        None
    }

    fn set_by_key<'a, K, D>(&mut self, keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        K::Item: Key,
        D: serde::Deserializer<'a>,
    {
        if let Some(inner) = self.0.as_mut() {
            inner.set_by_key(keys, de)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn get_by_key<K, S>(&self, keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Iterator,
        K::Item: Key,
        S: serde::Serializer,
    {
        if let Some(inner) = self.0.as_ref() {
            inner.get_by_key(keys, ser)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn metadata() -> Metadata {
        T::metadata()
    }

    fn traverse_by_key<K, F, E>(keys: K, func: F) -> Result<usize, Error<E>>
    where
        K: Iterator,
        K::Item: Key,
        F: FnMut(usize, &str) -> Result<(), E>,
    {
        T::traverse_by_key(keys, func)
    }
}

impl<T: serde::Serialize + serde::de::DeserializeOwned> Miniconf for core::option::Option<T> {
    fn name_to_index(_value: &str) -> core::option::Option<usize> {
        None
    }

    fn set_by_key<'a, K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Iterator,
        D: serde::Deserializer<'a>,
    {
        if keys.next().is_some() {
            return Err(Error::TooLong(0));
        }

        if let Some(inner) = self.as_mut() {
            *inner = serde::Deserialize::deserialize(de)?;
            Ok(0)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn get_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Iterator,
        S: serde::Serializer,
    {
        if keys.next().is_some() {
            return Err(Error::TooLong(0));
        }

        if let Some(inner) = self.as_ref() {
            serde::Serialize::serialize(inner, ser)?;
            Ok(0)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }

    fn traverse_by_key<K, F, E>(_keys: K, _func: F) -> Result<usize, Error<E>>
    where
        F: FnMut(usize, &str) -> Result<(), E>,
    {
        Ok(0)
    }
}
