//! Optional Settings Support
//!
//! # Design
//!
//! Miniconf supports optional settings trees. These are handled via the [`OptionalSetting`] type.
//! If the `OptionalSetting` is `None`, the field does not exist at run-time. It will not be
//! iterated over and cannot be `get()` or `set()` using the Miniconf API.
//!
//! This is intended as a mechanism to provide run-time construction of the structure. In some
//! cases, run-time detection may indicate that some component is not present. In this case,
//! settings will not be exposed for it.
//!
//!
//! # Standard Options
//!
//! Miniconf also allows for the normal usage of Rust `Option` types. In this case, the `Option`
//! can be used to atomically access the nullable content within.
use super::{Error, Miniconf, MiniconfMetadata};

pub struct OptionalSetting<T: Miniconf>(pub Option<T>);

impl<T: Miniconf> core::ops::Deref for OptionalSetting<T> {
    type Target = Option<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl<T: Miniconf> core::ops::DerefMut for OptionalSetting<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Default + Miniconf> Default for OptionalSetting<T> {
    fn default() -> Self {
        Self(Option::<T>::default())
    }
}

impl<T: core::fmt::Debug + Miniconf> core::fmt::Debug for OptionalSetting<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: PartialEq + Miniconf> PartialEq for OptionalSetting<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl<T: Clone + Miniconf> Clone for OptionalSetting<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T: Copy + Miniconf> Copy for OptionalSetting<T> {}

impl<T: Miniconf> Miniconf for OptionalSetting<T> {
    fn string_set(
        &mut self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        self.0.as_mut().map_or(Err(Error::PathNotFound), |inner| {
            inner.string_set(topic_parts, value)
        })
    }

    fn string_get(
        &self,
        topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        self.0.as_ref().map_or(Err(Error::PathNotFound), |inner| {
            inner.string_get(topic_parts, value)
        })
    }

    fn get_metadata(&self) -> MiniconfMetadata {
        self.0
            .as_ref()
            .map(|value| value.get_metadata())
            .unwrap_or_default()
    }

    fn recurse_paths<const TS: usize>(
        &self,
        index: &mut [usize],
        topic: &mut heapless::String<TS>,
    ) -> Option<()> {
        self.0
            .as_ref()
            .and_then(|value| value.recurse_paths(index, topic))
    }
}

impl<T: crate::Serialize + crate::DeserializeOwned> Miniconf for Option<T> {
    fn string_set(
        &mut self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &[u8],
    ) -> Result<(), Error> {
        if topic_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        *self = serde_json_core::from_slice(value)?.0;
        Ok(())
    }

    fn string_get(
        &self,
        mut topic_parts: core::iter::Peekable<core::str::Split<char>>,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        if topic_parts.peek().is_some() {
            return Err(Error::PathTooLong);
        }

        serde_json_core::to_slice(self, value).map_err(|_| Error::SerializationFailed)
    }

    fn get_metadata(&self) -> MiniconfMetadata {
        MiniconfMetadata {
            max_topic_size: 0,
            max_depth: 1,
        }
    }

    fn recurse_paths<const TS: usize>(
        &self,
        index: &mut [usize],
        _topic: &mut heapless::String<TS>,
    ) -> Option<()> {
        if index[0] == 0 {
            index[0] += 1;
            if self.is_some() {
                return Some(());
            }
        }

        None
    }
}
