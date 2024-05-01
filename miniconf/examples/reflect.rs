use core::any::{Any, TypeId};
use miniconf::{JsonPath, TreeAny, TreeKey};

#[non_exhaustive]
pub struct Caster<T: ?Sized> {
    pub ref_: fn(from: &dyn Any) -> Option<&T>,
    pub mut_: fn(from: &mut dyn Any) -> Option<&mut T>,
}

macro_rules! casters {
    ( $ty:ty => $trait:path ) => {(
        TypeId::of::<$ty>(),
        &Caster::<dyn $trait> {
            ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
            mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
        },
    )};
    ( $( $ty:ty ),+ => $trait:path ) => {(
        TypeId::of::<dyn $trait>(),
        &[ $( casters!($ty => $trait) ),+ ],
    )};
}

pub struct Registry<'a> {
    // use a suitable hashmap (maybe phf+linkme when TypeId::of() is const),
    // boxes, vecs, lazy caster creation, and a static where possible
    casters: &'a [(TypeId, &'a [(TypeId, &'a dyn Any)])],
}

impl<'a> Registry<'a> {
    pub fn caster<T: ?Sized + 'static>(&self, any: &dyn Any) -> Option<&Caster<T>> {
        let target = TypeId::of::<T>();
        let (_, types) = self
            .casters
            .iter()
            .find(|(trait_id, _)| trait_id == &target)?;
        let source = any.type_id();
        let (_, caster) = types.iter().find(|(type_id, _)| type_id == &source)?;
        caster.downcast_ref()
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
    use core::fmt::{Debug, Formatter, Write};
    let registry = Registry {
        casters: &[
            casters!(u8, i32, String, &[u8], &str => Debug),
            casters!(String, Formatter => Write),
            // casters!(erased_serde::Serialize => u8, i32, String, Vec<u8>),
        ],
    };

    let mut s = Settings::default();
    s.i[1].a = 9;

    let key: JsonPath = ".i[1].a".into();
    let a_any = s.ref_any_by_key(key).unwrap();
    let a: &dyn Debug = registry.cast_ref(a_any).unwrap();
    println!("{a:?}");
}
