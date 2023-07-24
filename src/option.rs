use super::{Error, IterError, Metadata, Miniconf};
use core::{
    fmt::Write,
    ops::{Deref, DerefMut},
};

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
    fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        if let Some(inner) = self.0.as_mut() {
            inner.set_path(path_parts, de)
        } else {
            Err(Error::PathAbsent)
        }
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        if let Some(inner) = self.0.as_ref() {
            inner.get_path(path_parts, ser)
        } else {
            Err(Error::PathAbsent)
        }
    }

    fn metadata() -> Metadata {
        T::metadata()
    }

    fn next_path(
        state: &[usize],
        depth: usize,
        path: impl Write,
        separator: char,
    ) -> Result<usize, IterError> {
        T::next_path(state, depth, path, separator)
    }
}

impl<T: crate::Serialize + crate::DeserializeOwned> Miniconf for core::option::Option<T> {
    fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), Error>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        if self.is_none() {
            return Err(Error::PathAbsent);
        }

        *self = Some(serde::Deserialize::deserialize(de).map_err(|_| Error::Deserialization)?);
        Ok(())
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        let data = self.as_ref().ok_or(Error::PathAbsent)?;
        serde::Serialize::serialize(data, ser).map_err(|_| Error::Serialization)
    }

    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }

    fn next_path(
        state: &[usize],
        depth: usize,
        _path: impl Write,
        _separator: char,
    ) -> Result<usize, IterError> {
        match state.get(depth) {
            Some(0) => Ok(depth),
            Some(_) => Err(IterError::Next(depth)),
            None => Err(IterError::Depth),
        }
    }
}
