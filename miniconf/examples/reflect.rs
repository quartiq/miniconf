use core::any::{Any, TypeId};
use miniconf::{JsonPath, TreeAny, TreeKey};
use std::{rc::Rc, sync::Arc};

#[non_exhaustive]
pub struct Caster<T: ?Sized> {
    pub ref_: fn(from: &dyn Any) -> Option<&T>,
    pub mut_: fn(from: &mut dyn Any) -> Option<&mut T>,
    pub box_: fn(from: Box<dyn Any>) -> Result<Box<T>, Box<dyn Any>>,
    pub box_send: fn(from: Box<dyn Any + Send>) -> Result<Box<T>, Box<dyn Any + Send>>,
    pub box_sync:
        fn(from: Box<dyn Any + Send + Sync>) -> Result<Box<T>, Box<dyn Any + Send + Sync>>,
    pub rc: fn(from: Rc<dyn Any>) -> Result<Rc<T>, Rc<dyn Any>>,
    pub arc: fn(from: Arc<dyn Any + Sync + Send>) -> Result<Arc<T>, Arc<dyn Any + Sync + Send>>,
}

macro_rules! casters {
    ($trait:path: $ty:ty) => {(
        TypeId::of::<$ty>(),
        &Caster::<dyn $trait> {
            ref_: |any| any.downcast_ref::<$ty>().map(|t| t as _),
            mut_: |any| any.downcast_mut::<$ty>().map(|t| t as _),
            box_: |any| any.downcast::<$ty>().map(|t| t as _),
            box_send: |any| any.downcast::<$ty>().map(|t| t as _),
            box_sync: |any| any.downcast::<$ty>().map(|t| t as _),
            rc: |any| any.downcast::<$ty>().map(|t| t as _),
            arc: |any| Err(any), // any.downcast::<$ty>().map(|t| t as _),
        },
    )};
    ($trait:path => $( $ty:ty ),+ ) => {(
        TypeId::of::<dyn $trait>(),
        &[ $( casters!($trait: $ty) ),+ ]
    )};
}

pub struct Registry<'a> {
    // use a suitable hashmap (maybe phf+linkme when TypeId::of() is const),
    // boxes, vecs, lazy caster creation, and a static where possible
    casters: &'a [(TypeId, &'a [(TypeId, &'a dyn Any)])],
}

impl<'a> Registry<'a> {
    pub fn caster<'b, T: ?Sized + 'static>(&self, any: &dyn Any) -> Option<&Caster<T>> {
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

    pub fn cast_box<'b, T: ?Sized + 'static>(
        &self,
        any: Box<dyn Any>,
    ) -> Result<Box<T>, Box<dyn Any>> {
        match self.caster(&*any) {
            Some(c) => (c.box_)(any),
            None => Err(any),
        }
    }

    pub fn cast_box_send<'b, T: ?Sized + 'static>(
        &self,
        any: Box<dyn Any + Send>,
    ) -> Result<Box<T>, Box<dyn Any + Send>> {
        match self.caster(&*any) {
            Some(c) => (c.box_send)(any),
            None => Err(any),
        }
    }

    pub fn cast_box_sync<'b, T: ?Sized + 'static>(
        &self,
        any: Box<dyn Any + Send + Sync>,
    ) -> Result<Box<T>, Box<dyn Any + Send + Sync>> {
        match self.caster(&*any) {
            Some(c) => (c.box_sync)(any),
            None => Err(any),
        }
    }

    pub fn cast_rc<'b, T: ?Sized + 'static>(&self, any: Rc<dyn Any>) -> Result<Rc<T>, Rc<dyn Any>> {
        match self.caster(&*any) {
            Some(c) => (c.rc)(any),
            None => Err(any),
        }
    }

    pub fn cast_arc<'b, T: ?Sized + 'static>(
        &self,
        any: Arc<dyn Any + Sync + Send>,
    ) -> Result<Arc<T>, Arc<dyn Any + Sync + Send>> {
        match self.caster(&*any) {
            Some(c) => (c.arc)(any),
            None => Err(any),
        }
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
    use core::fmt::{Debug, Display, Formatter, Write};
    let registry = Registry {
        casters: &[
            casters!(Display => u8, i32, String, &str),
            casters!(Debug => u8, i32, String, &[u8], &str),
            casters!(Write => String, Formatter),
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
