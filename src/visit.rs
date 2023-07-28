// pub enum Error<E> {
//     TooShort,
//     NotFound,
//     Inner(E),
// }

// impl<E> From<E> for Error<E> {
//     fn from(value: E) -> Self {
//         Error::Inner(value)
//     }
// }

// pub trait Walk {
//     fn name_to_index(value: &str) -> Option<usize>;

//     fn walk<K, V>(&self, keys: K, visitor: V) -> Result<V::Ok, Error<V::Error>>
//     where
//         K: Iterator,
//         K::Item: Key,
//         V: Visitor;
// }

// pub trait Key {
//     fn find<M: Walk>(self) -> Option<usize>;
// }

// impl Key for usize {
//     fn find<M>(self) -> Option<usize> {
//         Some(self)
//     }
// }

// impl Key for &str {
//     fn find<M: Walk>(self) -> Option<usize> {
//         M::name_to_index(self)
//     }
// }

// impl<T: Visit, const N: usize> Walk for [T; N] {
//     fn name_to_index(value: &str) -> Option<usize> {
//         value.parse().ok()
//     }

//     fn walk<K, V>(&self, mut keys: K, visitor: V) -> Result<V::Ok, Error<V::Error>>
//     where
//         K: Iterator,
//         K::Item: Key,
//         V: Visitor,
//     {
//         let key = keys.next().ok_or(Error::TooShort)?;
//         let index = key.find::<Self>().ok_or(Error::NotFound)?;
//         let item = self.get(index).ok_or(Error::NotFound)?;
//         Ok(item.visit(visitor)?)
//     }
// }



use paste::paste;

pub trait Visit {
    fn visit<'a, V>(&'a self, visitor: V) -> Result<V::Ok, V::Error>
    where
        V: Visitor<&'a Self>;
}

macro_rules! visit_primitive {
    ($($ty:ident)+) => { $( paste!{
        impl Visit for $ty {
            #[inline]
            fn visit<'a, V>(&self, visitor: V) -> Result<V::Ok, V::Error>
            where
                V: Visitor<&'a Self>,
            {
                visitor.[<visit_ $ty>](&self)
            }
        }
    } )+ }
}

visit_primitive!(
    bool isize i8 i16 i32 i64 usize u8 u16 u32 u64 f32 f64 char str
);

impl<I> Visit for [I] {
    #[inline]
    fn visit<'a, V>(&'a self, visitor: V) -> Result<V::Ok, V::Error>
    where
        V: Visitor<&'a I>,
    {
        visitor.visit_slice(self)
    }
}

// impl<I> Visit for Option<I> {
//     #[inline]
//     fn visit<'a, V>(&'a self, visitor: V) -> Result<V::Ok, V::Error>
//     where
//         V: Visitor<&'a I>,
//     {
//         visitor.visit_option(&self)
//     }
// }


macro_rules! visit_ref_primitive {
    ($($ty:ident)+) => { $( paste!{
        fn [<visit_ $ty>](self, _v: &$ty) -> Result<Self::Ok, Self::Error> {
            unimplemented!()
        }
    } )+ }
}

pub trait Visitor<I> where Self: Sized {
    type Ok;
    type Error;

    visit_ref_primitive!(
        bool isize i8 i16 i32 i64 usize u8 u16 u32 u64 f32 f64 char str
    );

    fn visit_slice(self, _v: &[I]) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }

    fn visit_option(self, _v: &Option<I>) -> Result<Self::Ok, Self::Error> {
        unimplemented!()
    }
}

macro_rules! visit_ref_forward {
    ($fn:path: $($ty:ident)+) => { $( paste!{
        #[inline]
        fn [<visit_ $ty>](self, v: &$ty) -> Result<Self::Ok, Self::Error> {
            $fn(v, self)
        }
    } )+ }
}

impl<T, I> Visitor<I> for T
where
    T: serde::Serializer,
    I: serde::Serialize,
{
    type Ok = T::Ok;
    type Error = T::Error;

    visit_ref_forward!(serde::Serialize::serialize:
        bool isize i8 i16 i32 i64 usize u8 u16 u32 u64 f32 f64 char str
    );

    #[inline]
    fn visit_slice(self, v: &[I]) -> Result<Self::Ok, Self::Error>
    {
        serde::Serialize::serialize(v, self)
    }

    #[inline]
    fn visit_option(self, v: &Option<I>) -> Result<Self::Ok, Self::Error>
    {
        serde::Serialize::serialize(v, self)
    }
}
