use core::any::{Any, TypeId};
use miniconf::{JsonPath, TreeAny, TreeKey};

struct Caster<T: ?Sized + Any> {
    cast_ref: fn(from: &dyn Any) -> &T,
    cast_mut: fn(from: &mut dyn Any) -> &mut T,
    // std: Box<T>, Rc<T>, Arc<T>
}

macro_rules! casters {
    ($trait:path: $ty:ty) => {(
        TypeId::of::<$ty>(),
        &Caster::<dyn $trait> {
            cast_ref: |from| from.downcast_ref::<$ty>().unwrap(),
            cast_mut: |from| from.downcast_mut::<$ty>().unwrap(),
        }
    )};
    ($trait:path => $($ty:ty),+) => {(
        TypeId::of::<dyn $trait>(),
        &[ $(casters!($trait: $ty)),+ ]
    )};
}

pub struct Registry<'a> {
    // use a suitable hashmap (maybe phf+linkme when TypeId::of() is const),
    // boxes, vecs, lazy caster creation, and a static where possible
    kv: &'a [(TypeId, &'a [(TypeId, &'a dyn Any)])],
}

impl<'a> Registry<'a> {
    fn caster<'b, T: ?Sized + 'static>(&self, any: &'b dyn Any) -> Option<&Caster<T>> {
        let id = TypeId::of::<T>();
        self.kv
            .iter()
            .find(|(trait_id, _)| trait_id == &id)
            .and_then(|(_, types)| {
                let id = (&*any).type_id();
                types
                    .iter()
                    .find(|(type_id, _)| type_id == &id)
                    .map(|(_, caster)| caster.downcast_ref().unwrap())
            })
    }

    pub fn implements<T: ?Sized + 'static>(&self, any: &dyn Any) -> bool {
        self.caster::<T>(any).is_some()
    }

    pub fn cast_ref<'b, T: ?Sized + 'static>(&self, any: &'b dyn Any) -> Option<&'b T> {
        self.caster(any).map(|caster| (caster.cast_ref)(any))
    }

    pub fn cast_mut<'b, T: ?Sized + 'static>(&self, any: &'b mut dyn Any) -> Option<&'b mut T> {
        self.caster(any).map(|caster| (caster.cast_mut)(any))
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
        kv: &[
            casters!(Display => u8, i32, String),
            casters!(Debug => u8, i32, String, &[u8]),
            casters!(Write => String, Formatter),
            // casters!(erased_serde::Serialize => ...),
            // casters!(erased_serde::Deerialize => ...),
        ],
    };

    let mut s = Settings::default();
    s.i[1].a = 9;

    let key: JsonPath = ".i[1].a".into();
    let a_any = s.ref_any_by_key(key).unwrap();
    let a: &dyn Debug = registry.cast_ref(a_any).unwrap();
    println!("{a:?}");
}
