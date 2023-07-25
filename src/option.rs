use super::{Error, Inner, IterError, Metadata, Miniconf, Outer};
use core::fmt::Write;

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

/// local type alias to minimize rename drama for now: FIXME
pub type Option<T> = core::option::Option<T>;

impl<T: Miniconf<Outer>> Miniconf<Inner> for core::option::Option<T> {
    fn set_path<'a, 'b: 'a, P, D>(
        &mut self,
        path_parts: &mut P,
        de: D,
    ) -> Result<(), Error<D::Error>>
    where
        P: Iterator<Item = &'a str>,
        D: serde::Deserializer<'b>,
    {
        if let Some(inner) = self.as_mut() {
            inner.set_path(path_parts, de)
        } else {
            Err(Error::PathAbsent)
        }
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        if let Some(inner) = self.as_ref() {
            inner.get_path(path_parts, ser)
        } else {
            Err(Error::PathAbsent)
        }
    }

    fn metadata(separator_length: usize) -> Metadata {
        T::metadata(separator_length)
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

impl<T: crate::Serialize + crate::DeserializeOwned> Miniconf<Outer> for core::option::Option<T> {
    fn set_path<'a, 'b: 'a, P, D>(
        &mut self,
        path_parts: &mut P,
        de: D,
    ) -> Result<(), Error<D::Error>>
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

        *self = Some(serde::Deserialize::deserialize(de)?);
        Ok(())
    }

    fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, Error<S::Error>>
    where
        P: Iterator<Item = &'a str>,
        S: serde::Serializer,
    {
        if path_parts.next().is_some() {
            return Err(Error::PathTooLong);
        }

        let data = self.as_ref().ok_or(Error::PathAbsent)?;
        Ok(serde::Serialize::serialize(data, ser)?)
    }

    fn metadata(_separator_length: usize) -> Metadata {
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
