#![cfg_attr(not(any(test, doctest, feature = "std")), no_std)]
#![cfg_attr(feature = "used_linker", feature(used_with_arg))]
#![cfg_attr(feature = "std", doc = include_str!("../README.md"))]
#![cfg_attr(
    not(feature = "std"),
    doc = "Cast from `dyn Any` to other trait objects"
)]
#![deny(rust_2018_compatibility)]
#![deny(rust_2018_idioms)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

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
///
/// With `downcast_unchecked()` the fields would all be exactly the same code: storing
/// the new vtable in the fat pointer. There would not be a `Caster<T>` generic struct
/// and the registry would be a double map of `{type_id: {trait_id: vtable_addr}}`.
#[doc(hidden)]
#[allow(clippy::type_complexity)]
#[derive(Clone, Debug, PartialEq)]
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

/// Key-value pair of the type registry.
///
/// `TypeId::of()` is not const (https://github.com/rust-lang/rust/issues/77125)
/// Hence we store a maker fn and call lazily.
/// Once it is const, remove the `fn() ->` and `||` and investigate phf.
/// The caster is a static ref to a trait object since its type
/// varies with the target trait.
#[doc(hidden)]
pub type Entry<'a> = (fn() -> [TypeId; 2], &'a (dyn Any + Send + Sync));

/// Static slice of key maker fns and Caster trait objects
#[doc(hidden)]
#[linkme::distributed_slice]
pub static REGISTRY_KV: [Entry<'static>];

/// Register a type and target trait in the registry
#[macro_export]
macro_rules! register {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::register! { $crate::REGISTRY_KV: $ty => $tr $(, $flag)? }
    };
    ( $reg:path: $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::gensym! { $crate::register!{ $reg: $ty => $tr $(, $flag)? } }
    };
    ( $name:ident, $reg:path: $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        #[$crate::linkme::distributed_slice($reg)]
        #[linkme(crate=$crate::linkme)]
        static $name: $crate::Entry<'static> = $crate::entry!( $ty => $tr $(, $flag)? );
    };
}

/// Build an entry for the type-trait registry
#[macro_export]
macro_rules! entry {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        (
            (|| [::core::any::TypeId::of::<$tr>(), ::core::any::TypeId::of::<$ty>()]) as _,
            &$crate::caster!( $ty => $tr $(, $flag)? ) as _,
        )
    };
}

/// Build a `Caster` for a given concrete type and target trait
#[cfg(all(not(feature = "alloc"), not(feature = "std")))]
#[macro_export]
#[doc(hidden)]
macro_rules! caster {
    ( $ty:ty => $tr:ty $(, $flag:ident)? ) => {
        $crate::Caster::<$tr> {
            ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
            mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
        }
    };
}
/// Build a `Caster` for a given concrete type and target trait
#[cfg(all(feature = "alloc", not(feature = "std")))]
#[macro_export]
#[doc(hidden)]
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
#[cfg(feature = "std")]
#[macro_export]
#[doc(hidden)]
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
    ( $ty:ty, no_arc ) => {
        |any| Err(any)
    };
}

/// The type-trait registry
#[cfg(not(feature = "std"))]
#[derive(Default, Debug, Clone)]
pub struct Registry<'a>(
    // TODO: fixed size map, ~6kB RAM
    // Note that each key is size_of([TypeId; 2]) = 32 bytes.
    // Maybe risk collisions and reduce key size, reduce hash size, or use LinearMap
    heapless::FnvIndexMap<[TypeId; 2], &'a (dyn Any + Send + Sync), { 1 << 7 }>,
);

/// The type-trait registry
#[cfg(feature = "std")]
#[derive(Default, Debug, Clone)]
pub struct Registry<'a>(std::collections::HashMap<[TypeId; 2], &'a (dyn Any + Send + Sync)>);

impl<'a> Registry<'a> {
    /// Build a new registry from an iterator over key (`fn() -> [TypeId; 2]`)
    /// value (`&'a dyn Any + Send + Sync`) pairs.
    pub fn new<'b>(it: impl IntoIterator<Item = &'b Entry<'a>>) -> Self
    where
        'a: 'b,
    {
        Self(
            it.into_iter()
                .map(|(key, value)| ((*key)(), *value))
                .collect(),
        )
    }

    /// Obtain a `Caster` given the TypeId for the concrete type and a target trait `T`
    fn caster<T: ?Sized + 'static>(&self, any: TypeId) -> Option<&Caster<T>> {
        self.0.get(&[TypeId::of::<T>(), any])?.downcast_ref()
    }

    /// Whether the `Any` can be cast to the target trait T
    pub fn castable_ref<T: ?Sized + 'static>(&self, any: &dyn Any) -> bool {
        self.0.contains_key(&[TypeId::of::<T>(), any.type_id()])
    }

    /// Whether the concrete type U can be case to the target trait T
    pub fn castable<T: ?Sized + 'static, U: ?Sized + 'static>(&self) -> bool {
        self.0.contains_key(&[TypeId::of::<T>(), TypeId::of::<U>()])
    }

    /// Cast to an immuatbly borrowed trait object
    pub fn cast_ref<'b, T: ?Sized + 'static>(&self, any: &'b dyn Any) -> Option<&'b T> {
        (self.caster(any.type_id())?.ref_)(any)
    }

    /// Cast to a mutably borrowed trait object
    pub fn cast_mut<'b, T: ?Sized + 'static>(&self, any: &'b mut dyn Any) -> Option<&'b mut T> {
        (self.caster((*any).type_id())?.mut_)(any)
    }

    /// Cast to a boxed trait object
    #[cfg(feature = "alloc")]
    pub fn cast_box<T: ?Sized + 'static>(&self, any: Box<dyn Any>) -> Result<Box<T>, Box<dyn Any>> {
        match self.caster((*any).type_id()) {
            Some(c) => (c.box_)(any),
            None => Err(any),
        }
    }

    /// Cast to a ref counted trait object
    #[cfg(feature = "std")]
    pub fn cast_rc<T: ?Sized + 'static>(&self, any: Rc<dyn Any>) -> Result<Rc<T>, Rc<dyn Any>> {
        match self.caster((*any).type_id()) {
            Some(c) => (c.rc)(any),
            None => Err(any),
        }
    }

    /// Cast to an atomically ref counted trait object
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

/// The global static type-trait registry
pub static REGISTRY: Lazy<Registry<'static>> = Lazy::new(|| Registry::new(REGISTRY_KV));

/// Whether a `dyn Any` can be cast to a given trait object
pub trait CastableRef {
    /// Whether we can be cast to a given trait object
    fn castable<T: ?Sized + 'static>(self) -> bool;
}

impl CastableRef for &dyn Any {
    fn castable<T: ?Sized + 'static>(self) -> bool {
        REGISTRY.castable_ref::<T>(self)
    }
}

/// Whether this concrete type can be cast to a given trait object
pub trait Castable {
    /// Whether this type is castable to the given trait object
    fn castable<T: ?Sized + 'static>() -> bool;
}

impl<U: ?Sized + 'static> Castable for U {
    fn castable<T: ?Sized + 'static>() -> bool {
        REGISTRY.castable::<T, U>()
    }
}

/// Cast an `dyn Any` to another given trait object
///
/// Uses the global type-trait registry.
pub trait Cast<T> {
    /// Cast a `dyn Any` (reference or smart pointer) to a given trait object
    fn cast(self) -> Option<T>;
}

impl<'b, T: ?Sized + 'static> Cast<&'b T> for &'b dyn Any {
    fn cast(self) -> Option<&'b T> {
        REGISTRY.cast_ref(self)
    }
}

impl<'b, T: ?Sized + 'static> Cast<&'b mut T> for &'b mut dyn Any {
    fn cast(self) -> Option<&'b mut T> {
        REGISTRY.cast_mut(self)
    }
}

#[cfg(feature = "alloc")]
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
