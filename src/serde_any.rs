use crate::{Error, Miniconf};
use core::any::Any;
use paste::paste;

#[derive(Copy, Debug, Clone)]
pub struct E;
impl core::fmt::Display for E {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}
impl serde::de::Error for E {
    fn custom<T>(msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        E
    }
}
impl serde::ser::Error for E {
    fn custom<T>(msg: T) -> Self
    where
        T: std::fmt::Display,
    {
        E
    }
}

pub struct SerdeAny {
    data: Option<Box<dyn Any>>,
}

macro_rules! ser {
    ($($name:ident)+) => {
        $( paste! {
            fn [<serialize_ $name>](mut self, v: $name) -> Result<Self::Ok, Self::Error> {
                self.data = Some(Box::new(v));
                Ok(())
            }
        })+
    };
}

macro_rules! de {
    ($($name:ident)+) => {
        $( paste!{
            fn [<deserialize_ $name>]<V>(mut self, visitor: V) -> Result<V::Value, Self::Error>
            where
                V: serde::de::Visitor<'de>,
            {
                visitor.[<visit_ $name>](*self.data.take().unwrap().downcast().unwrap())
            }
        })+
    };
}

impl serde::Serializer for SerdeAny {
    type Error = E;
    type Ok = ();
    fn is_human_readable(&self) -> bool {
        false
    }
    ser!(bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char);
}

impl<'de> serde::Deserializer<'de> for SerdeAny {
    type Error = E;
    de!(bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str);
}

pub trait TreeAny<const Y: usize = 1>: Miniconf<Y> {
    fn set_any<'a>(&mut self, path: &str, data: Box<dyn Any + 'a>) -> Result<(), Error<E>>;
    fn get_any<'a>(&mut self, path: &str) -> Result<Box<dyn Any + 'a>, Error<E>>;
}

impl<T: Miniconf<Y>, const Y: usize> TreeAny<Y> for T {
    fn set_any<'a>(&mut self, path: &str, data: Box<dyn Any + 'a>) -> Result<(), Error<E>> {
        let mut de = SerdeAny { data: Some(data) };
        self.set_by_key(path.split('/').skip(1), de)?;
        Ok(())
    }

    fn get_any<'a>(&mut self, path: &str) -> Result<Box<dyn Any + 'a>, Error<E>> {
        let mut ser = SerdeAny { data: None };
        self.get_by_key(path.split('/').skip(1), ser)?;
        Ok(ser.data.unwrap())
    }
}
