use core::{
    any::Any,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Keys, Schema, SerdeError, TreeAny, TreeDeserialize, TreeSchema, TreeSerialize, ValueError,
};

/// Handler module for leaf fields without [`Leaf`] newtype
///
/// To be used as a derive macros attribute `#[tree(with=miniconf::leaf)]`.
pub mod leaf {
    use super::*;

    /// [`TreeSchema::SCHEMA`]
    pub const SCHEMA: &'static Schema = &Schema::LEAF;

    /// [`TreeSerialize::serialize_by_key()`]
    pub fn serialize_by_key<T: Serialize + ?Sized, S: Serializer>(
        value: &T,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        value.serialize(ser).map_err(SerdeError::Inner)
    }

    /// [`TreeDeserialize::deserialize_by_key()`]
    pub fn deserialize_by_key<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        value: &mut T,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        *value = T::deserialize(de).map_err(SerdeError::Inner)?;
        Ok(())
    }

    /// [`TreeDeserialize::probe_by_key()`]
    pub fn probe_by_key<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        T::deserialize(de).map_err(SerdeError::Inner)?;
        Ok(())
    }

    /// [`TreeAny::ref_any_by_key()`]
    pub fn ref_any_by_key(value: &impl Any, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        keys.finalize()?;
        Ok(value)
    }

    /// [`TreeAny::mut_any_by_key()`]
    pub fn mut_any_by_key(
        value: &mut impl Any,
        mut keys: impl Keys,
    ) -> Result<&mut dyn Any, ValueError> {
        keys.finalize()?;
        Ok(value)
    }
}

/// `Serialize`/`Deserialize`/`Any` leaf
///
/// This wraps [`Serialize`], [`Deserialize`], and [`Any`] into `Tree` a leaf node.
///
/// ```
/// use miniconf::{json, Leaf, Tree};
/// let mut s = Leaf(0);
/// json::set(&mut s, "", b"7").unwrap();
/// assert!(matches!(*s, 7));
/// ```
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Leaf<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for Leaf<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for Leaf<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: Display> Display for Leaf<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl<T: ?Sized> TreeSchema for Leaf<T> {
    const SCHEMA: &'static Schema = leaf::SCHEMA;
}

impl<T: Serialize + ?Sized> TreeSerialize for Leaf<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        leaf::serialize_by_key(&self.0, keys, ser)
    }
}

impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Leaf<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        leaf::deserialize_by_key(&mut self.0, keys, de)
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        leaf::probe_by_key::<T, _>(keys, de)
    }
}

impl<T: Any> TreeAny for Leaf<T> {
    #[inline]
    fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
        leaf::ref_any_by_key(&self.0, keys)
    }

    #[inline]
    fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        leaf::mut_any_by_key(&mut self.0, keys)
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

macro_rules! impl_leaf {
    ($ty0:ty, $($ty:ty), +) => {
        impl_leaf! {$ty0}
        impl_leaf! {$($ty),+}
    };
    ($ty:ty) => {
        impl TreeSchema for $ty {
            const SCHEMA: &'static Schema = leaf::SCHEMA;
        }

        impl TreeSerialize for $ty {
            #[inline]
            fn serialize_by_key<S: Serializer>(
                &self,
                keys: impl Keys,
                ser: S,
            ) -> Result<S::Ok, SerdeError<S::Error>> {
                leaf::serialize_by_key(self, keys, ser)
            }
        }

        impl<'de> TreeDeserialize<'de> for $ty {
            #[inline]
            fn deserialize_by_key<D: Deserializer<'de>>(
                &mut self,
                keys: impl Keys,
                de: D,
            ) -> Result<(), SerdeError<D::Error>> {
                leaf::deserialize_by_key(self, keys, de)
            }

            #[inline]
            fn probe_by_key<D: Deserializer<'de>>(
                keys: impl Keys,
                de: D,
            ) -> Result<(), SerdeError<D::Error>> {
                leaf::probe_by_key::<Self, _>(keys, de)
            }
        }

        impl TreeAny for $ty {
            #[inline]
            fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
                leaf::ref_any_by_key(self, keys)
            }

            #[inline]
            fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
                leaf::mut_any_by_key(self, keys)
            }
        }
    };
}

impl_leaf! {
    (), bool, char, f32, f64,
    i8, i16, i32, i64, i128, isize,
    u8, u16, u32, u64, u128, usize
}
use core::sync::atomic::{
    AtomicBool, AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicIsize, AtomicU16, AtomicU32,
    AtomicU64, AtomicU8, AtomicUsize,
};
impl_leaf! {
    AtomicBool,
    AtomicI8, AtomicI16, AtomicI32, AtomicI64, AtomicIsize,
    AtomicU8, AtomicU16, AtomicU32, AtomicU64, AtomicUsize
}
use core::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
impl_leaf! {SocketAddr, SocketAddrV4, SocketAddrV6}
impl_leaf! {core::time::Duration}

macro_rules! impl_unsized_leaf {
    ($ty:ty) => {
        impl TreeSchema for $ty {
            const SCHEMA: &'static Schema = leaf::SCHEMA;
        }

        impl TreeSchema for &$ty {
            const SCHEMA: &'static Schema = leaf::SCHEMA;
        }

        impl TreeSerialize for $ty {
            #[inline]
            fn serialize_by_key<S: Serializer>(
                &self,
                keys: impl Keys,
                ser: S,
            ) -> Result<S::Ok, SerdeError<S::Error>> {
                leaf::serialize_by_key(self, keys, ser)
            }
        }

        impl<'a, 'de: 'a> TreeDeserialize<'de> for &'a $ty {
            #[inline]
            fn deserialize_by_key<D: Deserializer<'de>>(
                &mut self,
                keys: impl Keys,
                de: D,
            ) -> Result<(), SerdeError<D::Error>> {
                leaf::deserialize_by_key(self, keys, de)
            }

            #[inline]
            fn probe_by_key<D: Deserializer<'de>>(
                keys: impl Keys,
                de: D,
            ) -> Result<(), SerdeError<D::Error>> {
                leaf::probe_by_key::<Self, _>(keys, de)
            }
        }
    };
}

impl_unsized_leaf! {str}
impl_unsized_leaf! {[u8]}

#[cfg(feature = "alloc")]
impl_leaf! {String}

#[cfg(feature = "std")]
mod std_impls {
    use std::{
        ffi::{CString, OsString},
        path::{Path, PathBuf},
        time::SystemTime,
    };

    use super::*;

    impl_leaf! {CString, OsString}
    impl_leaf! {PathBuf}
    impl_leaf! {SystemTime}
    impl_unsized_leaf! {Path}
}

#[cfg(feature = "heapless")]
mod heapless_impls {
    use super::*;
    use heapless::String;

    impl<const N: usize> TreeSchema for String<N> {
        const SCHEMA: &'static Schema = leaf::SCHEMA;
    }

    impl<const N: usize> TreeSerialize for String<N> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerdeError<S::Error>> {
            leaf::serialize_by_key(self, keys, ser)
        }
    }

    impl<'de, const N: usize> TreeDeserialize<'de> for String<N> {
        #[inline]
        fn deserialize_by_key<D: Deserializer<'de>>(
            &mut self,
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerdeError<D::Error>> {
            leaf::deserialize_by_key(self, keys, de)
        }

        #[inline]
        fn probe_by_key<D: Deserializer<'de>>(
            keys: impl Keys,
            de: D,
        ) -> Result<(), SerdeError<D::Error>> {
            leaf::probe_by_key::<String<N>, _>(keys, de)
        }
    }

    impl<const N: usize> TreeAny for String<N> {
        #[inline]
        fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
            leaf::ref_any_by_key(self, keys)
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            leaf::mut_any_by_key(self, keys)
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////

// TODO: port to module

/// `TryFrom<&str>`/`AsRef<str>` leaf
///
/// This wraps [`TryFrom<&str>`] and [`AsRef<str>`] into a `Tree*` leaf.
/// [`TreeAny`] is implemented but denied access at runtime.
/// It is especially useful to support enum variant switching using `strum`.
/// Inner enum variant field access can be implemented using `defer`.
///
/// ```
/// use miniconf::{json, Leaf, StrLeaf, Tree};
/// #[derive(Tree, strum::AsRefStr, strum::EnumString)]
/// enum En {
///     A(Leaf<i32>),
///     B(Leaf<f32>),
/// }
/// #[derive(Tree)]
/// struct S {
///     e: StrLeaf<En>,
///     #[tree(typ="En", defer=(*self.e))]
///     t: (),
/// }
/// let mut s = S {
///     e: StrLeaf(En::A(Leaf(9))),
///     t: (),
/// };
/// json::set(&mut s, "/e", b"\"B\"").unwrap();
/// json::set(&mut s, "/t/B", b"1.2").unwrap();
/// assert!(matches!(*s.e, En::B(Leaf(1.2))));
/// ```
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct StrLeaf<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for StrLeaf<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for StrLeaf<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: ?Sized> TreeSchema for StrLeaf<T> {
    const SCHEMA: &'static Schema = &Schema::LEAF;
}

impl<T: AsRef<str> + ?Sized> TreeSerialize for StrLeaf<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        let name = self.0.as_ref();
        name.serialize(ser).map_err(SerdeError::Inner)
    }
}

impl<'de, T: TryFrom<&'de str>> TreeDeserialize<'de> for StrLeaf<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(SerdeError::Inner)?;
        self.0 = T::try_from(name).or(Err(ValueError::Access("Could not convert from str")))?;
        Ok(())
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        let name = Deserialize::deserialize(de).map_err(SerdeError::Inner)?;
        T::try_from(name).or(Err(ValueError::Access("Could not convert from str")))?;
        Ok(())
    }
}

impl<T> TreeAny for StrLeaf<T> {
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        keys.finalize()?;
        Err(ValueError::Access("No Any access for StrLeaf"))
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        keys.finalize()?;
        Err(ValueError::Access("No Any access for StrLeaf"))
    }
}

impl<T: Display> Display for StrLeaf<T> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

// TODO: remove

/// Deny any value access
#[derive(
    Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Deny<T: ?Sized>(pub T);

impl<T: ?Sized> Deref for Deny<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for Deny<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: TreeSchema + ?Sized> TreeSchema for Deny<T> {
    const SCHEMA: &'static Schema = T::SCHEMA;
}

impl<T: TreeSchema + ?Sized> TreeSerialize for Deny<T> {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        _keys: impl Keys,
        _ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        Err(ValueError::Access("Denied").into())
    }
}

impl<'de, T: TreeSchema + ?Sized> TreeDeserialize<'de> for Deny<T> {
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        _keys: impl Keys,
        _de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        Err(ValueError::Access("Denied").into())
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        _keys: impl Keys,
        _de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        Err(ValueError::Access("Denied").into())
    }
}

impl<T: TreeSchema + ?Sized> TreeAny for Deny<T> {
    #[inline]
    fn ref_any_by_key(&self, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
        Err(ValueError::Access("Denied"))
    }

    #[inline]
    fn mut_any_by_key(&mut self, _keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        Err(ValueError::Access("Denied"))
    }
}

// TODO: remove

/// (Draft) An integer with a limited range of valid values
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct RangeLeaf<T: ?Sized, const MIN: isize, const MAX: isize>(T);

impl<T, const MIN: isize, const MAX: isize> Default for RangeLeaf<T, MIN, MAX>
where
    T: Copy + Default + TryInto<isize> + TryFrom<isize>,
{
    fn default() -> Self {
        assert!(MIN <= MAX);
        Self(
            T::default()
                .try_into()
                .ok()
                .unwrap_or(MIN + (MAX - MIN) / 2)
                .max(MIN)
                .min(MAX)
                .try_into()
                .ok()
                .unwrap(),
        )
    }
}

impl<T: ?Sized, const MIN: isize, const MAX: isize> Deref for RangeLeaf<T, MIN, MAX> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Copy + TryInto<isize>, const MIN: isize, const MAX: isize> RangeLeaf<T, MIN, MAX> {
    /// The range of valid values
    pub const RANGE: core::ops::RangeInclusive<isize> = MIN..=MAX;

    /// Create a new RangeLeaf
    #[inline]
    pub fn new(value: T) -> Option<Self> {
        Some(Self(Self::check(value).ok()?))
    }

    /// Check and set the inner value
    #[inline]
    pub fn set(&mut self, value: T) -> Option<T> {
        self.0 = Self::check(value).ok()?;
        Some(self.0)
    }

    fn check(value: T) -> Result<T, ValueError> {
        let v = value
            .try_into()
            .or(Err(ValueError::Access("Can't convert")))?;
        if Self::RANGE.contains(&v) {
            Ok(value)
        } else {
            Err(ValueError::Access("Out of range"))
        }
    }

    /// Extract just the inner
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T, const MIN: isize, const MAX: isize> TreeSchema for RangeLeaf<T, MIN, MAX> {
    const SCHEMA: &'static Schema = &Schema {
        meta: Some(&[
            // FIXME const_format
            ("min", stringify!(MIN)),
            ("max", stringify!(MAX)),
        ]),
        internal: None,
    };
}

impl<T: Serialize + TryInto<isize> + Copy, const MIN: isize, const MAX: isize> TreeSerialize
    for RangeLeaf<T, MIN, MAX>
{
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        Self::check(self.0)?
            .serialize(ser)
            .map_err(SerdeError::Inner)
    }
}

impl<'de, T: Deserialize<'de> + TryInto<isize> + Copy, const MIN: isize, const MAX: isize>
    TreeDeserialize<'de> for RangeLeaf<T, MIN, MAX>
{
    #[inline]
    fn deserialize_by_key<D: Deserializer<'de>>(
        &mut self,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        self.0 = Self::check(T::deserialize(de).map_err(SerdeError::Inner)?)?;
        Ok(())
    }

    #[inline]
    fn probe_by_key<D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        Self::check(T::deserialize(de).map_err(SerdeError::Inner)?)?;
        Ok(())
    }
}

impl<T: Any + TryInto<isize> + Copy, const MIN: isize, const MAX: isize> TreeAny
    for RangeLeaf<T, MIN, MAX>
{
    #[inline]
    fn ref_any_by_key(&self, mut keys: impl Keys) -> Result<&dyn Any, ValueError> {
        keys.finalize()?;
        Self::check(self.0)?;
        Ok(&self.0)
    }

    #[inline]
    fn mut_any_by_key(&mut self, mut keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
        keys.finalize()?;
        Err(ValueError::Access("No unchecked mutable borrow"))
    }
}
