use core::fmt;

use serde::de::{self, Visitor};
use serde::forward_to_deserialize_any;
use serde::ser::{self, Impossible};
use serde::{Deserializer, Serializer};

pub const RESPONSE_CAPACITY: usize = 24;

#[derive(Copy, Clone, Debug)]
pub enum CodecError {
    Overflow,
    Parse,
    Unsupported,
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Overflow => f.write_str("overflow"),
            Self::Parse => f.write_str("parse"),
            Self::Unsupported => f.write_str("unsupported"),
        }
    }
}

impl ser::Error for CodecError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Self::Unsupported
    }
}

impl de::Error for CodecError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        Self::Parse
    }
}

#[derive(Copy, Clone)]
pub struct Response {
    buf: [u8; RESPONSE_CAPACITY],
    len: usize,
}

impl Response {
    pub const fn new() -> Self {
        Self {
            buf: [0; RESPONSE_CAPACITY],
            len: 0,
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn push_byte(&mut self, byte: u8) -> Result<(), CodecError> {
        if self.len >= self.buf.len() {
            return Err(CodecError::Overflow);
        }
        self.buf[self.len] = byte;
        self.len += 1;
        Ok(())
    }

    fn push_bytes(&mut self, bytes: &[u8]) -> Result<(), CodecError> {
        if self.len + bytes.len() > self.buf.len() {
            return Err(CodecError::Overflow);
        }
        self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        Ok(())
    }

    pub fn write_bool(&mut self, value: bool) -> Result<(), CodecError> {
        if value {
            self.push_bytes(b"true")
        } else {
            self.push_bytes(b"false")
        }
    }

    pub fn write_u8(&mut self, value: u8) -> Result<(), CodecError> {
        self.write_u32(value as u32)
    }

    pub fn write_i32(&mut self, value: i32) -> Result<(), CodecError> {
        if value < 0 {
            self.push_byte(b'-')?;
        }
        self.write_u32(value.unsigned_abs())
    }

    pub fn write_option_i32(&mut self, value: Option<i32>) -> Result<(), CodecError> {
        match value {
            Some(v) => self.write_i32(v),
            None => self.push_bytes(b"null"),
        }
    }

    fn write_u32(&mut self, mut value: u32) -> Result<(), CodecError> {
        let mut tmp = [0u8; 10];
        let mut n = 0usize;
        loop {
            tmp[n] = b'0' + (value % 10) as u8;
            value /= 10;
            n += 1;
            if value == 0 {
                break;
            }
        }
        for d in tmp[..n].iter().rev() {
            self.push_byte(*d)?;
        }
        Ok(())
    }
}

impl Default for Response {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ResponseSerializer<'a> {
    out: &'a mut Response,
}

impl<'a> ResponseSerializer<'a> {
    pub fn new(out: &'a mut Response) -> Self {
        out.clear();
        Self { out }
    }
}

impl Serializer for ResponseSerializer<'_> {
    type Ok = ();
    type Error = CodecError;
    type SerializeSeq = Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = Impossible<Self::Ok, Self::Error>;
    type SerializeMap = Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.out.write_bool(v)
    }

    fn serialize_i8(self, _v: i8) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_i16(self, _v: i16) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.out.write_i32(v)
    }
    fn serialize_i64(self, _v: i64) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.out.write_u8(v)
    }
    fn serialize_u16(self, _v: u16) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_u32(self, _v: u32) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_u64(self, _v: u64) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_f32(self, _v: f32) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_f64(self, _v: f64) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_char(self, _v: char) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_str(self, _v: &str) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_bytes(self, _v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.out.push_bytes(b"null")
    }
    fn serialize_some<T: ?Sized + serde::Serialize>(
        self,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_newtype_struct<T: ?Sized + serde::Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_newtype_variant<T: ?Sized + serde::Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Err(CodecError::Unsupported)
    }
    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(CodecError::Unsupported)
    }

    fn collect_str<T: ?Sized + fmt::Display>(self, _value: &T) -> Result<Self::Ok, Self::Error> {
        Err(CodecError::Unsupported)
    }
}

pub struct ValueDeserializer<'a> {
    input: &'a str,
}

impl<'a> ValueDeserializer<'a> {
    pub const fn new(input: &'a str) -> Self {
        Self { input }
    }
}

impl<'de> Deserializer<'de> for ValueDeserializer<'de> {
    type Error = CodecError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(CodecError::Unsupported)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.input {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(CodecError::Parse),
        }
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v: i32 = self.input.parse().map_err(|_| CodecError::Parse)?;
        visitor.visit_i32(v)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let v: u8 = self.input.parse().map_err(|_| CodecError::Parse)?;
        visitor.visit_u8(v)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.input == "null" {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(CodecError::Unsupported)
    }

    forward_to_deserialize_any! {
        i8 i16 i64 i128 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        unit_struct newtype_struct seq tuple tuple_struct map struct enum identifier ignored_any
    }
}

pub fn parse_bool(input: &str) -> Result<bool, CodecError> {
    match input {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CodecError::Parse),
    }
}

pub fn parse_i32(input: &str) -> Result<i32, CodecError> {
    input.parse::<i32>().map_err(|_| CodecError::Parse)
}

pub fn parse_u8(input: &str) -> Result<u8, CodecError> {
    input.parse::<u8>().map_err(|_| CodecError::Parse)
}

pub fn parse_option_i32(input: &str) -> Result<Option<i32>, CodecError> {
    if input == "null" {
        Ok(None)
    } else {
        Ok(Some(parse_i32(input)?))
    }
}
