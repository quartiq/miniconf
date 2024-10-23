use core::{
    any::Any,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{Error, Keys, Traversal, TreeAny, TreeDeserialize, TreeKey, TreeSerialize, Walk};

/// `TryFrom<&str>`/`AsRef<str>` set-by-name
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ByName<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for ByName<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for ByName<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> ByName<T> {
    /// Extract just the inner
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for ByName<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: ?Sized> TreeKey for ByName<T> {
    fn traverse_all<W: Walk>() -> Result<W, W::Error> {
        Ok(W::leaf())
    }

    fn traverse_by_key<K, F, E>(mut keys: K, _func: F) -> Result<usize, Error<E>>
    where
        K: Keys,
        F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
    {
        keys.finalize()?;
        Ok(0)
    }
}

impl<T: AsRef<str> + ?Sized> TreeSerialize for ByName<T> {
    fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, Error<S::Error>>
    where
        K: Keys,
        S: Serializer,
    {
        keys.finalize()?;
        self.0
            .as_ref()
            .serialize(ser)
            .map_err(|err| Error::Inner(0, err))?;
        Ok(0)
    }
}

impl<'de, T: TryFrom<&'de str>> TreeDeserialize<'de> for ByName<T> {
    fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, Error<D::Error>>
    where
        K: Keys,
        D: Deserializer<'de>,
    {
        keys.finalize()?;
        let name: &str = Deserialize::deserialize(de).map_err(|err| Error::Inner(0, err))?;
        self.0 = T::try_from(name).or(Err(Traversal::Invalid(0, "Invalid name")))?;
        Ok(0)
    }
}

impl<T: Any> TreeAny for ByName<T> {
    fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Ok(&self.0)
    }

    fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn Any, Traversal>
    where
        K: Keys,
    {
        keys.finalize()?;
        Ok(&mut self.0)
    }
}
