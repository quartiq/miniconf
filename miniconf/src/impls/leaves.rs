use core::{
    any::Any,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    Keys, Schema, SerdeError, TreeAny, TreeDeserialize, TreeSchema, TreeSerialize, ValueError,
};

/// Passthrough Tree*
pub mod passthrough {
    use super::*;

    /// [`TreeSerialize::serialize_by_key()`]
    #[inline]
    pub fn serialize_by_key<T: TreeSerialize + ?Sized, S: Serializer>(
        value: &T,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        value.serialize_by_key(keys, ser)
    }

    /// [`TreeDeserialize::deserialize_by_key()`]
    #[inline]
    pub fn deserialize_by_key<'de, T: TreeDeserialize<'de> + ?Sized, D: Deserializer<'de>>(
        value: &mut T,
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        value.deserialize_by_key(keys, de)
    }

    /// [`TreeDeserialize::probe_by_key()`]
    #[inline]
    pub fn probe_by_key<'de, T: TreeDeserialize<'de> + ?Sized, D: Deserializer<'de>>(
        keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        T::probe_by_key(keys, de)
    }

    /// [`TreeAny::ref_any_by_key()`]
    #[inline]
    pub fn ref_any_by_key(
        value: &(impl TreeAny + ?Sized),
        keys: impl Keys,
    ) -> Result<&dyn Any, ValueError> {
        value.ref_any_by_key(keys)
    }

    /// [`TreeAny::mut_any_by_key()`]
    #[inline]
    pub fn mut_any_by_key(
        value: &mut (impl TreeAny + ?Sized),
        keys: impl Keys,
    ) -> Result<&mut dyn Any, ValueError> {
        value.mut_any_by_key(keys)
    }
}

/// Leaf implementation using serde::{Serialize, Deserialize}
///
/// To be used as a derive macros attribute `#[tree(with=leaf)]`.
pub mod leaf {
    use super::*;

    /// [`TreeSchema::SCHEMA`]
    pub const SCHEMA: &Schema = &Schema::LEAF;

    /// [`TreeSerialize::serialize_by_key()`]
    pub fn serialize_by_key<T: Serialize + ?Sized, S: Serializer>(
        value: &T,
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        Serialize::serialize(value, ser).map_err(SerdeError::Inner)
    }

    /// [`TreeDeserialize::deserialize_by_key()`]
    pub fn deserialize_by_key<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        value: &mut T,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        Deserialize::deserialize_in_place(de, value).map_err(SerdeError::Inner)?;
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
/// use miniconf::{json_core, Leaf, Tree};
/// let mut s = Leaf(0);
/// json_core::set(&mut s, "", b"7").unwrap();
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
        leaf::probe_by_key::<Self, _>(keys, de)
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
    ($($ty:ty),*) => {$(
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
    )*};
}

impl_leaf! {
    (), bool, char, f32, f64,
    i8, i16, i32, i64, i128, isize,
    u8, u16, u32, u64, u128, usize
}
impl_leaf! {core::net::SocketAddr, core::net::SocketAddrV4, core::net::SocketAddrV6}
impl_leaf! {core::time::Duration}

macro_rules! impl_unsized_leaf {
    ($($ty:ty),*) => {$(
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
    )*};
}

impl_unsized_leaf! {str}

impl<T> TreeSchema for [T] {
    const SCHEMA: &'static Schema = leaf::SCHEMA;
}

impl<T: Serialize> TreeSerialize for [T] {
    #[inline]
    fn serialize_by_key<S: Serializer>(
        &self,
        keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        leaf::serialize_by_key(self, keys, ser)
    }
}

impl<'a, 'de: 'a, T> TreeDeserialize<'de> for &'a [T]
where
    &'a [T]: Deserialize<'de>,
{
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

#[cfg(feature = "alloc")]
mod alloc_impls {
    use super::*;

    use alloc::{string::String, vec::Vec};

    impl_leaf! {String}

    impl<T> TreeSchema for Vec<T> {
        const SCHEMA: &'static Schema = leaf::SCHEMA;
    }

    impl<T: Serialize> TreeSerialize for Vec<T> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerdeError<S::Error>> {
            leaf::serialize_by_key(self, keys, ser)
        }
    }

    impl<'de, T: Deserialize<'de>> TreeDeserialize<'de> for Vec<T> {
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

    impl<T: 'static> TreeAny for Vec<T> {
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

#[cfg(feature = "std")]
mod std_impls {
    use super::*;

    impl_leaf! {std::ffi::CString, std::ffi::OsString}
    impl_leaf! {std::time::SystemTime}
    impl_leaf! {std::path::PathBuf}
    impl_unsized_leaf! {std::path::Path}

    #[cfg(target_has_atomic = "8")]
    impl_leaf! { core::sync::atomic::AtomicBool, core::sync::atomic::AtomicI8, core::sync::atomic::AtomicU8 }
    #[cfg(target_has_atomic = "16")]
    impl_leaf! { core::sync::atomic::AtomicI16, core::sync::atomic::AtomicU16 }
    #[cfg(target_has_atomic = "32")]
    impl_leaf! { core::sync::atomic::AtomicI32, core::sync::atomic::AtomicU32 }
    #[cfg(target_has_atomic = "64")]
    impl_leaf! { core::sync::atomic::AtomicI64, core::sync::atomic::AtomicU64 }
}

#[cfg(feature = "heapless")]
mod heapless_impls {
    use super::*;

    use heapless::{String, Vec};

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
            leaf::probe_by_key::<Self, _>(keys, de)
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

    impl<T, const N: usize> TreeSchema for Vec<T, N> {
        const SCHEMA: &'static Schema = leaf::SCHEMA;
    }

    impl<T: Serialize, const N: usize> TreeSerialize for Vec<T, N> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerdeError<S::Error>> {
            leaf::serialize_by_key(self, keys, ser)
        }
    }

    impl<'de, T: Deserialize<'de>, const N: usize> TreeDeserialize<'de> for Vec<T, N> {
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

    impl<T: 'static, const N: usize> TreeAny for Vec<T, N> {
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

#[cfg(feature = "heapless-09")]
mod heapless_09_impls {
    use super::*;

    use heapless_09::{
        LenType, String, Vec,
        string::{StringInner, StringStorage},
        vec::{VecInner, VecStorage},
    };

    impl<LenT: LenType, O: StringStorage + ?Sized> TreeSchema for StringInner<LenT, O> {
        const SCHEMA: &'static Schema = leaf::SCHEMA;
    }

    impl<LenT: LenType, O: StringStorage + ?Sized> TreeSerialize for StringInner<LenT, O> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerdeError<S::Error>> {
            leaf::serialize_by_key(self, keys, ser)
        }
    }

    impl<'de, const N: usize, LenT: LenType> TreeDeserialize<'de> for String<N, LenT> {
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

    impl<const N: usize, LenT: LenType + 'static> TreeAny for String<N, LenT> {
        #[inline]
        fn ref_any_by_key(&self, keys: impl Keys) -> Result<&dyn Any, ValueError> {
            leaf::ref_any_by_key(self, keys)
        }

        #[inline]
        fn mut_any_by_key(&mut self, keys: impl Keys) -> Result<&mut dyn Any, ValueError> {
            leaf::mut_any_by_key(self, keys)
        }
    }

    impl<T, LenT: LenType, O: VecStorage<T> + ?Sized> TreeSchema for VecInner<T, LenT, O> {
        const SCHEMA: &'static Schema = leaf::SCHEMA;
    }

    impl<T: Serialize, const N: usize, LenT: LenType> TreeSerialize for Vec<T, N, LenT> {
        #[inline]
        fn serialize_by_key<S: Serializer>(
            &self,
            keys: impl Keys,
            ser: S,
        ) -> Result<S::Ok, SerdeError<S::Error>> {
            leaf::serialize_by_key(self, keys, ser)
        }
    }

    impl<'de, T: Deserialize<'de>, const N: usize, LenT: LenType> TreeDeserialize<'de>
        for Vec<T, N, LenT>
    {
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

    impl<T: 'static, const N: usize, LenT: LenType + 'static> TreeAny for Vec<T, N, LenT> {
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

/// `TryFrom<&str>`/`AsRef<str>` leaf
///
/// This wraps [`TryFrom<&str>`] and [`AsRef<str>`] into a `Tree*` leaf.
/// [`TreeAny`] is implemented but denied access at runtime.
/// It is especially useful to support enum variant switching using `strum`.
/// Inner enum variant field access can be implemented using `defer`.
///
/// ```
/// use miniconf::{json_core::set, str_leaf, Tree};
/// #[derive(Tree, strum::AsRefStr, strum::EnumString)]
/// enum En {
///     A(i32),
///     B(f32),
/// }
/// #[derive(Tree)]
/// struct S {
///     #[tree(rename="t", with=str_leaf, defer=self.e, typ="En")]
///     _t: (),
///     e: En,
/// }
/// let mut s = S {
///     _t: (),
///     e: En::A(9),
/// };
/// set(&mut s, "/t", b"\"B\"").unwrap();
/// set(&mut s, "/e/B", b"1.2").unwrap();
/// assert!(matches!(s.e, En::B(1.2)));
/// ```
pub mod str_leaf {
    use super::*;

    pub use deny::{mut_any_by_key, ref_any_by_key};
    pub use leaf::SCHEMA;

    /// [`TreeSerialize::serialize_by_key()`]
    #[inline]
    pub fn serialize_by_key<S: Serializer>(
        value: &(impl AsRef<str> + ?Sized),
        mut keys: impl Keys,
        ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        keys.finalize()?;
        value.as_ref().serialize(ser).map_err(SerdeError::Inner)
    }

    /// [`TreeDeserialize::deserialize_by_key()`]
    #[inline]
    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        value: &mut impl TryFrom<&'de str>,
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        let name: &str = Deserialize::deserialize(de).map_err(SerdeError::Inner)?;
        *value = name
            .try_into()
            .or(Err(ValueError::Access("Could not convert from str")))?;
        Ok(())
    }

    /// [`TreeDeserialize::probe_by_key()`]
    #[inline]
    pub fn probe_by_key<'de, T: TryFrom<&'de str>, D: Deserializer<'de>>(
        mut keys: impl Keys,
        de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        keys.finalize()?;
        let name: &str = Deserialize::deserialize(de).map_err(SerdeError::Inner)?;
        T::try_from(name).or(Err(ValueError::Access("Could not convert from str")))?;
        Ok(())
    }
}

/// Deny access tools.
///
/// These return early without consuming keys or finalizing them.
pub mod deny {
    use super::*;

    pub use leaf::SCHEMA;

    /// [`TreeSerialize::serialize_by_key()`]
    #[inline]
    pub fn serialize_by_key<S: Serializer>(
        _value: &impl ?Sized,
        _keys: impl Keys,
        _ser: S,
    ) -> Result<S::Ok, SerdeError<S::Error>> {
        Err(ValueError::Access("Denied").into())
    }

    /// [`TreeDeserialize::deserialize_by_key()`]
    #[inline]
    pub fn deserialize_by_key<'de, D: Deserializer<'de>>(
        _value: &mut impl ?Sized,
        _keys: impl Keys,
        _de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        Err(ValueError::Access("Denied").into())
    }

    /// [`TreeDeserialize::probe_by_key()`]
    #[inline]
    pub fn probe_by_key<'de, T: ?Sized, D: Deserializer<'de>>(
        _keys: impl Keys,
        _de: D,
    ) -> Result<(), SerdeError<D::Error>> {
        Err(ValueError::Access("Denied").into())
    }

    /// [`TreeAny::ref_any_by_key()`]
    #[inline]
    pub fn ref_any_by_key(_value: &impl ?Sized, _keys: impl Keys) -> Result<&dyn Any, ValueError> {
        Err(ValueError::Access("Denied"))
    }

    /// [`TreeAny::mut_any_by_key()`]
    #[inline]
    pub fn mut_any_by_key(
        _value: &mut impl ?Sized,
        _keys: impl Keys,
    ) -> Result<&mut dyn Any, ValueError> {
        Err(ValueError::Access("Denied"))
    }
}
