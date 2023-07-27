use crate::{Error, Metadata, Miniconf, Ok, Result};
use core::ops::{Deref, DerefMut};

/// An `Option` that exposes its value through their [`Miniconf`] implementation.
///
/// # Design
///
/// Miniconf supports optional values in two forms.
///
/// In both forms, the `Option` may be marked with `#[miniconf(defer)]`
/// and be `None` at run-time. This makes the corresponding part of the namespace inaccessible
/// at run-time. It will still be iterated over by [`crate::SerDe::iter_paths()`] but cannot be
/// `get()` or `set()` using the [`Miniconf`] API.
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

impl<T: Miniconf> Miniconf for Option<T> {
    fn set_by_name<'a, 'b: 'a, P, D>(&mut self, names: &mut P, de: D) -> Result<D::Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        if let Some(inner) = self.0.as_mut() {
            inner.set_by_name(names, de)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn get_by_name<'a, P, S>(&self, names: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        if let Some(inner) = self.0.as_ref() {
            inner.get_by_name(names, ser)
        } else {
            Err(Error::Absent(0))
        }
    }

    fn metadata(separator_length: usize) -> Metadata {
        T::metadata(separator_length)
    }

    fn traverse_by_index<P, F, E>(indices: &mut P, func: F, internal: bool) -> Result<E>
    where
        P: Iterator<Item = usize>,
        F: FnMut(usize, &str) -> core::result::Result<(), E>,
    {
        T::traverse_by_index(indices, func, internal)
    }

    fn traverse_by_name<'a, P, F, E>(names: &mut P, func: F, internal: bool) -> Result<E>
    where
        P: Iterator<Item = &'a str>,
        F: FnMut(usize, &str) -> core::result::Result<(), E>,
    {
        T::traverse_by_name(names, func, internal)
    }
}

impl<T: serde::Serialize + serde::de::DeserializeOwned> Miniconf for core::option::Option<T> {
    fn set_by_name<'a, 'b: 'a, P, D>(&mut self, names: &mut P, de: D) -> Result<D::Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        if names.next().is_some() {
            return Err(Error::TooLong(0));
        }

        if self.is_none() {
            return Err(Error::Absent(0));
        }

        *self = Some(serde::Deserialize::deserialize(de)?);
        Ok(Ok::Leaf(0))
    }

    fn get_by_name<'a, P, S>(&self, names: &mut P, ser: S) -> Result<S::Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        if names.next().is_some() {
            return Err(Error::TooLong(0));
        }

        let data = self.as_ref().ok_or(Error::Absent(0))?;
        serde::Serialize::serialize(data, ser)?;
        Ok(Ok::Leaf(0))
    }

    fn metadata(_separator_length: usize) -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }

    fn traverse_by_index<P, F, E>(_indices: &mut P, _func: F, _internal: bool) -> Result<E>
    where
        P: Iterator<Item = usize>,
        F: FnMut(usize, &str) -> core::result::Result<(), E>,
    {
        Ok(Ok::Leaf(0))
    }

    fn traverse_by_name<'a, P, F, E>(_names: &mut P, _func: F, _internal: bool) -> Result<E>
    where
        P: Iterator<Item = &'a str>,
        F: FnMut(usize, &str) -> core::result::Result<(), E>,
    {
        Ok(Ok::Leaf(0))
    }
}
