use super::{Error, IterError, Metadata, Miniconf, Peekable};
use core::ops::{Deref, DerefMut};

/// An `Option` that exposes its value through their [`Miniconf`] implementation.
///
/// # Design
///
/// Miniconf supports optional values in two forms. The first for is the [`miniconf::Option`](Option)
/// type. If the `Option` is `None`, the part of the namespace does not exist at run-time.
/// It will not be iterated over and cannot be `get()` or `set()` using the [`Miniconf`] API.
///
/// This is intended as a mechanism to provide run-time construction of the namespace. In some
/// cases, run-time detection may indicate that some component is not present. In this case,
/// namespaces will not be exposed for it.
///
/// Miniconf also allows for the normal usage of Rust [`core::option::Option`] types. In this case,
/// the `Option` can be used to atomically access the nullable content within if marked with the
/// `#[miniconf(defer)]` attribute.
/// 
/// # Construction
/// 
/// The `miniconf::Option` can be constructed using [`From<core::option::Option>`]/[`Into<miniconf::Option>`]
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
    fn set_path<'a, P: Peekable<Item = &'a str>>(
        &mut self,
        path_parts: &'a mut P,
        value: &[u8],
    ) -> Result<usize, Error> {
        if let Some(inner) = self.0.as_mut() {
            inner.set_path(path_parts, value)
        } else {
            Err(Error::PathNotFound)
        }
    }

    fn get_path<'a, P: Peekable<Item = &'a str>>(
        &self,
        path_parts: &'a mut P,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        if let Some(inner) = self.0.as_ref() {
            inner.get_path(path_parts, value)
        } else {
            Err(Error::PathNotFound)
        }
    }

    fn metadata() -> Metadata {
        T::metadata()
    }

    fn next_path<const TS: usize>(
        state: &mut [usize],
        path: &mut heapless::String<TS>,
    ) -> Result<bool, IterError> {
        T::next_path(state, path)
    }
}

impl<T: crate::Serialize + crate::DeserializeOwned> Miniconf for core::option::Option<T> {
    fn set_path<'a, P: Peekable<Item = &'a str>>(
        &mut self,
        path_parts: &mut P,
        value: &[u8],
    ) -> Result<usize, Error> {
        if path_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        if self.is_none() {
            return Err(Error::PathAbsent);
        }

        let (value, len) = serde_json_core::from_slice(value)?;
        *self = Some(value);
        Ok(len)
    }

    fn get_path<'a, P: Peekable<Item = &'a str>>(
        &self,
        path_parts: &mut P,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        if path_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        let data = self.as_ref().ok_or(Error::PathAbsent)?;
        Ok(serde_json_core::to_slice(data, value)?)
    }

    fn metadata() -> Metadata {
        Metadata {
            count: 1,
            ..Default::default()
        }
    }

    fn next_path<const TS: usize>(
        state: &mut [usize],
        path: &mut heapless::String<TS>,
    ) -> Result<bool, IterError> {
        if *state.first().ok_or(IterError::PathDepth)? == 0 {
            state[0] += 1;

            // Remove trailing slash added by a deferring container (array or struct).
            if path.ends_with('/') {
                path.pop();
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
