//! Option Support
//!
//! # Design
//!
//! Miniconf supports optional values in two forms. The first for is the [`Option`] type. If the
//! `Option` is `None`, the part of the namespace does not exist at run-time.
//! It will not be iterated over and cannot be `get()` or `set()` using the Miniconf API.
//!
//! This is intended as a mechanism to provide run-time construction of the namespace. In some
//! cases, run-time detection may indicate that some component is not present. In this case,
//! namespaces will not be exposed for it.
//!
//!
//! # Standard Options
//!
//! Miniconf also allows for the normal usage of Rust `Option` types. In this case, the `Option`
//! can be used to atomically access the nullable content within.
use super::{Error, Metadata, Miniconf, Peekable};

/// An `Option` that exposes its value through their [`Miniconf`](trait.Miniconf.html) implementation.
pub struct Option<T>(pub core::option::Option<T>);

impl<T> core::ops::Deref for Option<T> {
    type Target = core::option::Option<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T> core::ops::DerefMut for Option<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Default> Default for Option<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for Option<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq> PartialEq for Option<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Clone> Clone for Option<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy> Copy for Option<T> {}

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

    fn next_path<const TS: usize>(state: &mut [usize], path: &mut heapless::String<TS>) -> bool {
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

        // Note(unwrap): checked above
        let data = self.as_ref().ok_or(Error::PathAbsent)?;
        serde_json_core::to_slice(data, value).map_err(|_| Error::SerializationFailed)
    }

    fn metadata() -> Metadata {
        Metadata::default()
    }

    fn next_path<const TS: usize>(state: &mut [usize], path: &mut heapless::String<TS>) -> bool {
        if state[0] == 0 {
            state[0] += 1;

            // Remove trailing slash added by a deferring container (array or struct).
            if path.ends_with('/') {
                path.pop();
            }
            true
        } else {
            false
        }
    }
}
