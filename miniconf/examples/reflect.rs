use core::any::{Any, TypeId};
use heapless::FnvIndexMap;
use miniconf::{JsonPath, TreeAny, TreeKey};
use once_cell::sync::Lazy;

#[non_exhaustive]
pub struct Caster<T: ?Sized> {
    pub ref_: fn(from: &dyn Any) -> Option<&T>,
    pub mut_: fn(from: &mut dyn Any) -> Option<&mut T>,
}

type Entry = ([TypeId; 2], &'static (dyn Any + Send + Sync));

#[::linkme::distributed_slice]
static __REGISTRY: [fn() -> Entry];

macro_rules! register {
    ( $ty:ty, $tr:ty ) => {
        register! { __REGISTRY, $ty, $tr }
    };
    ( $reg:ident, $ty:ty, $tr:ty ) => {
        ::gensym::gensym! { register!{ $reg, $ty, $tr } }
    };
    ( $name:ident, $reg:ident, $ty:ty, $tr:ty ) => {
        #[::linkme::distributed_slice($reg)]
        fn $name() -> Entry {
            (
                [
                    ::core::any::TypeId::of::<$tr>(),
                    ::core::any::TypeId::of::<$ty>(),
                ],
                &Caster::<$tr> {
                    ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
                    mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
                },
            )
        }
    };
}

pub struct Registry<const N: usize>(FnvIndexMap<[TypeId; 2], &'static (dyn Any + Send + Sync), N>);

impl<const N: usize> Registry<N> {
    pub fn caster<T: ?Sized + 'static>(&self, any: &dyn Any) -> Option<&Caster<T>> {
        self.0
            .get(&[TypeId::of::<T>(), any.type_id()])?
            .downcast_ref()
    }

    pub fn implements<T: ?Sized + 'static>(&self, any: &dyn Any) -> bool {
        self.caster::<T>(any).is_some()
    }

    pub fn cast_ref<'b, T: ?Sized + 'static>(&self, any: &'b dyn Any) -> Option<&'b T> {
        (self.caster(any)?.ref_)(any)
    }

    pub fn cast_mut<'b, T: ?Sized + 'static>(&self, any: &'b mut dyn Any) -> Option<&'b mut T> {
        (self.caster(any)?.mut_)(any)
    }
}

static REGISTRY: Lazy<Registry<128>> =
    Lazy::new(|| Registry(__REGISTRY.iter().map(|maker| maker()).collect()));

trait Cast<T> {
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

use core::fmt::Debug;
use core::ops::AddAssign;

register! { u8, dyn Debug }
register! { i32, dyn AddAssign<i32> + Sync }
register! { i32, dyn erased_serde::Serialize }

#[derive(TreeKey, TreeAny, Default)]
struct Inner {
    a: u8,
}

#[derive(TreeKey, TreeAny, Default)]
struct Settings {
    v: i32,
    #[tree(depth = 2)]
    i: [Inner; 2],
}

fn main() {
    let mut s = Settings::default();
    s.i[1].a = 9;

    let key: JsonPath = ".i[1].a".into();
    let a: &dyn Debug = s.ref_any_by_key(key).unwrap().cast().unwrap();
    println!("{a:?}");

    let key: JsonPath = ".v".into();
    let v: &mut (dyn AddAssign<i32> + Sync) = s.mut_any_by_key(key).unwrap().cast().unwrap();
    *v += 3;
    assert_eq!(s.v, 3);
}
