#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(feature = "std", doc = include_str!("../README.md"))]

use core::any::{Any, TypeId};

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::boxed::Box;
#[cfg(feature = "std")]
use std::{rc::Rc, sync::Arc};

use once_cell::sync::Lazy;

// Re-exports for `register!()` macro
#[doc(hidden)]
pub use gensym::gensym;
#[doc(hidden)]
pub use linkme;

/// Contains all relevant casting functions for a type-trait pair
/// The casting path is (conceptually) `Any::downcast::<Type>() as Target`.
///
/// The intermediate concrete type must be consistent.
#[doc(hidden)]
#[allow(clippy::type_complexity)]
pub struct Caster<T: ?Sized> {
    pub ref_: fn(&dyn Any) -> Option<&T>,
    pub mut_: fn(&mut dyn Any) -> Option<&mut T>,
    #[cfg(feature = "alloc")]
    pub box_: fn(Box<dyn Any>) -> Result<Box<T>, Box<dyn Any>>,
    #[cfg(feature = "std")]
    pub rc: fn(Rc<dyn Any>) -> Result<Rc<T>, Rc<dyn Any>>,
    #[cfg(feature = "std")]
    pub arc: fn(Arc<dyn Any + Sync + Send>) -> Result<Arc<T>, Arc<dyn Any + Sync + Send>>,
}

/// Key-value pair of the type registry
/// The caster is a static ref to a trait object since its type
/// varies with the target trait.
#[doc(hidden)]
pub type Entry = ([TypeId; 2], &'static (dyn Any + Send + Sync));

/// Static slice of key and caster maker functions.
/// `TypeId::of()` is not const
/// But luckily we can make the Caster 'static.
#[doc(hidden)]
#[linkme::distributed_slice]
pub static __REGISTRY: [fn() -> Entry];

/// Register a type and target trait in the registry.
#[macro_export]
macro_rules! register {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::register! { $crate::__REGISTRY: $ty => $tr $(, $flag)? }
    };
    ( $reg:path: $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::gensym! { $crate::register!{ $reg: $ty => $tr $(, $flag)? } }
    };
    ( $name:ident, $reg:path: $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        #[$crate::linkme::distributed_slice($reg)]
        #[linkme(crate=$crate::linkme)]
        fn $name() -> $crate::Entry {
            (
                [
                    ::core::any::TypeId::of::<$tr>(),
                    ::core::any::TypeId::of::<$ty>(),
                ],
                &$crate::caster!($ty => $tr $(, $flag)? ),
            )
        }
    };
}

/// Build a `Caster` for a given concrete type and target trait
#[cfg(all(feature = "alloc", not(feature = "std")))]
#[macro_export]
macro_rules! caster {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::Caster::<$tr> {
            ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
            mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
            box_: |any| any.downcast::<$ty>().map(|t| t as _),
        }
    };
}
/// Build a `Caster` for a given concrete type and target trait
#[cfg(all(not(feature = "alloc"), not(feature = "std")))]
#[macro_export]
macro_rules! caster {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::Caster::<$tr> {
            ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
            mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
        }
    };
}
/// Build a `Caster` for a given concrete type and target trait
#[cfg(feature = "std")]
#[macro_export]
macro_rules! caster {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::Caster::<$tr> {
            ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
            mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
            box_: |any| any.downcast::<$ty>().map(|t| t as _),
            rc: |any| any.downcast::<$ty>().map(|t| t as _),
            arc: $crate::caster!( $ty $(, $flag)? ),
        }
    };
    ( $ty:ty ) => {
        |any| any.downcast::<$ty>().map(|t| t as _)
    };
    ( $ty:ty, $flag:ident ) => {
        |any| Err(any)
    };
}

// TODO: fixed size map
// Note that each key is size_of([TypeId; 2]) = 32 bytes.
// Maybe risk type collisions and reduce key size.
#[cfg(not(feature = "std"))]
type RegistryMap = heapless::FnvIndexMap<[TypeId; 2], &'static (dyn Any + Send + Sync), { 1 << 7 }>;

#[cfg(feature = "std")]
type RegistryMap = std::collections::HashMap<[TypeId; 2], &'static (dyn Any + Send + Sync)>;

/// The type-trait registry.
#[derive(Default, Debug, Clone)]
pub struct Registry(pub RegistryMap);

impl Registry {
    /// Obtain a `Caster` given the TypeId for the concrete type and a target trait `T`.
    fn caster<T: ?Sized + 'static>(&self, any: TypeId) -> Option<&Caster<T>> {
        self.0.get(&[TypeId::of::<T>(), any])?.downcast_ref()
    }

    /// Whether the `Any` can be cast to the target trait.
    pub fn castable_ref<T: ?Sized + 'static>(&self, any: &dyn Any) -> bool {
        self.0.contains_key(&[TypeId::of::<T>(), any.type_id()])
    }

    /// Whether the concrete type can be case to the target trait.
    pub fn castable<T: ?Sized + 'static, U: ?Sized + 'static>(&self) -> bool {
        self.0.contains_key(&[TypeId::of::<T>(), TypeId::of::<U>()])
    }

    /// Cast to an immuatbly borrowed trait object.
    pub fn cast_ref<'b, T: ?Sized + 'static>(&self, any: &'b dyn Any) -> Option<&'b T> {
        (self.caster(any.type_id())?.ref_)(any)
    }

    /// Cast to a mutably borrowed trait object.
    pub fn cast_mut<'b, T: ?Sized + 'static>(&self, any: &'b mut dyn Any) -> Option<&'b mut T> {
        (self.caster((*any).type_id())?.mut_)(any)
    }

    /// Cast to a boxed trait object.
    #[cfg(feature = "alloc")]
    pub fn cast_box<T: ?Sized + 'static>(&self, any: Box<dyn Any>) -> Result<Box<T>, Box<dyn Any>> {
        match self.caster((*any).type_id()) {
            Some(c) => (c.box_)(any),
            None => Err(any),
        }
    }

    /// Cast to a ref counted trait object.
    #[cfg(feature = "std")]
    pub fn cast_rc<T: ?Sized + 'static>(&self, any: Rc<dyn Any>) -> Result<Rc<T>, Rc<dyn Any>> {
        match self.caster((*any).type_id()) {
            Some(c) => (c.rc)(any),
            None => Err(any),
        }
    }

    /// Cast to an atomically ref counted trait object.
    #[cfg(feature = "std")]
    pub fn cast_arc<T: ?Sized + 'static>(
        &self,
        any: Arc<dyn Any + Sync + Send>,
    ) -> Result<Arc<T>, Arc<dyn Any + Sync + Send>> {
        match self.caster((*any).type_id()) {
            Some(c) => (c.arc)(any),
            None => Err(any),
        }
    }
}

/// The global type registry
pub static REGISTRY: Lazy<Registry> =
    Lazy::new(|| Registry(__REGISTRY.iter().map(|maker| maker()).collect()));

/// Capability to determine whether we can be cast to a given trait object.
pub trait CastableRef {
    /// Whether we can be cast to a given trait object.
    fn castable<T: ?Sized + 'static>(self) -> bool;
}

impl CastableRef for &dyn Any {
    fn castable<T: ?Sized + 'static>(self) -> bool {
        REGISTRY.castable_ref::<T>(self)
    }
}

/// Whether `Any` trait objects of this concrete type can be cast to a given trait object.
pub trait Castable {
    /// This type is castable to the given trait object.
    fn castable<T: ?Sized + 'static>() -> bool;
}

impl<U: ?Sized + 'static> Castable for U {
    fn castable<T: ?Sized + 'static>() -> bool {
        REGISTRY.castable::<T, U>()
    }
}

/// Cast an `Any` trait object to another trait object.
pub trait Cast<T> {
    fn cast(self) -> Option<T>;
}

impl<'a, T: ?Sized + 'static> Cast<&'a T> for &'a dyn Any {
    fn cast(self) -> Option<&'a T> {
        REGISTRY.cast_ref(self)
    }
}

impl<'a, T: ?Sized + 'static> Cast<&'a mut T> for &'a mut dyn Any {
    fn cast(self) -> Option<&'a mut T> {
        REGISTRY.cast_mut(self)
    }
}

#[cfg(feature = "std")]
impl<T: ?Sized + 'static> Cast<Box<T>> for Box<dyn Any> {
    fn cast(self) -> Option<Box<T>> {
        REGISTRY.cast_box(self).ok()
    }
}

#[cfg(feature = "std")]
impl<T: ?Sized + 'static> Cast<Rc<T>> for Rc<dyn Any> {
    fn cast(self) -> Option<Rc<T>> {
        REGISTRY.cast_rc(self).ok()
    }
}

#[cfg(feature = "std")]
impl<T: ?Sized + 'static> Cast<Arc<T>> for Arc<dyn Any + Sync + Send> {
    fn cast(self) -> Option<Arc<T>> {
        REGISTRY.cast_arc(self).ok()
    }
}
