use crate::codec::{CodecError, Response, parse_bool, parse_i32, parse_option_i32, parse_u8};
use crate::settings::{MyEnum, Settings};

pub struct Engine {
    settings: Settings,
}

pub enum ManualError {
    InvalidPath,
    Absent,
    Unsupported,
    Codec(CodecError),
}

macro_rules! manual_leafs {
    ($m:ident) => {
        $m! {
            Foo, "/foo",
            |s: &Engine, o: &mut Response| o.write_bool(s.settings.foo).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.foo = parse_bool(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            EnumLeaf, "/enum_",
            |_s: &Engine, _o: &mut Response| Err(ManualError::Unsupported),
            |_s: &mut Engine, _i: &str| Err(ManualError::Unsupported);

            StructLeaf, "/struct_",
            |_s: &Engine, _o: &mut Response| Err(ManualError::Unsupported),
            |_s: &mut Engine, _i: &str| Err(ManualError::Unsupported);

            ArrayLeaf, "/array",
            |_s: &Engine, _o: &mut Response| Err(ManualError::Unsupported),
            |_s: &mut Engine, _i: &str| Err(ManualError::Unsupported);

            OptionLeaf, "/option",
            |s: &Engine, o: &mut Response| o.write_option_i32(s.settings.option).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.option = parse_option_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            UniLeaf, "/uni",
            |_s: &Engine, _o: &mut Response| Err(ManualError::Unsupported),
            |_s: &mut Engine, _i: &str| Err(ManualError::Unsupported);

            StructTreeA, "/struct_tree/a",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.struct_tree.a).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.struct_tree.a = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            StructTreeB, "/struct_tree/b",
            |s: &Engine, o: &mut Response| o.write_u8(s.settings.struct_tree.b).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.struct_tree.b = parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            EnumTreeA, "/enum_tree/A",
            |s: &Engine, o: &mut Response| match s.settings.enum_tree {
                MyEnum::A(v) => o.write_i32(v).map_err(ManualError::Codec),
                _ => Err(ManualError::Absent),
            },
            |s: &mut Engine, i: &str| match &mut s.settings.enum_tree {
                MyEnum::A(v) => {
                    *v = parse_i32(i).map_err(ManualError::Codec)?;
                    Ok(())
                }
                _ => Err(ManualError::Absent),
            };

            EnumTreeBA, "/enum_tree/B/a",
            |s: &Engine, o: &mut Response| match &s.settings.enum_tree {
                MyEnum::B(v) => o.write_i32(v.a).map_err(ManualError::Codec),
                _ => Err(ManualError::Absent),
            },
            |s: &mut Engine, i: &str| match &mut s.settings.enum_tree {
                MyEnum::B(v) => {
                    v.a = parse_i32(i).map_err(ManualError::Codec)?;
                    Ok(())
                }
                _ => Err(ManualError::Absent),
            };

            EnumTreeBB, "/enum_tree/B/b",
            |s: &Engine, o: &mut Response| match &s.settings.enum_tree {
                MyEnum::B(v) => o.write_u8(v.b).map_err(ManualError::Codec),
                _ => Err(ManualError::Absent),
            },
            |s: &mut Engine, i: &str| match &mut s.settings.enum_tree {
                MyEnum::B(v) => {
                    v.b = parse_u8(i).map_err(ManualError::Codec)?;
                    Ok(())
                }
                _ => Err(ManualError::Absent),
            };

            EnumTreeC0A, "/enum_tree/C/0/a",
            |s: &Engine, o: &mut Response| o.write_i32(s.enum_tree_get_c(0, true)?).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| s.enum_tree_set_c_i32(0, parse_i32(i).map_err(ManualError::Codec)?);

            EnumTreeC0B, "/enum_tree/C/0/b",
            |s: &Engine, o: &mut Response| o.write_u8(s.enum_tree_get_c(0, false)? as u8).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| s.enum_tree_set_c_u8(0, parse_u8(i).map_err(ManualError::Codec)?);

            EnumTreeC1A, "/enum_tree/C/1/a",
            |s: &Engine, o: &mut Response| o.write_i32(s.enum_tree_get_c(1, true)?).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| s.enum_tree_set_c_i32(1, parse_i32(i).map_err(ManualError::Codec)?);

            EnumTreeC1B, "/enum_tree/C/1/b",
            |s: &Engine, o: &mut Response| o.write_u8(s.enum_tree_get_c(1, false)? as u8).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| s.enum_tree_set_c_u8(1, parse_u8(i).map_err(ManualError::Codec)?);

            ArrayTree0, "/array_tree/0",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.array_tree[0]).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.array_tree[0] = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayTree1, "/array_tree/1",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.array_tree[1]).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.array_tree[1] = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayTree2_0A, "/array_tree2/0/a",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.array_tree2[0].a).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.array_tree2[0].a = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayTree2_0B, "/array_tree2/0/b",
            |s: &Engine, o: &mut Response| o.write_u8(s.settings.array_tree2[0].b).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.array_tree2[0].b = parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayTree2_1A, "/array_tree2/1/a",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.array_tree2[1].a).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.array_tree2[1].a = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayTree2_1B, "/array_tree2/1/b",
            |s: &Engine, o: &mut Response| o.write_u8(s.settings.array_tree2[1].b).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.array_tree2[1].b = parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            TupleTree0, "/tuple_tree/0",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.tuple_tree.0).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.tuple_tree.0 = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            TupleTree1A, "/tuple_tree/1/a",
            |s: &Engine, o: &mut Response| o.write_i32(s.settings.tuple_tree.1.a).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.tuple_tree.1.a = parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            TupleTree1B, "/tuple_tree/1/b",
            |s: &Engine, o: &mut Response| o.write_u8(s.settings.tuple_tree.1.b).map_err(ManualError::Codec),
            |s: &mut Engine, i: &str| {
                s.settings.tuple_tree.1.b = parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            OptionTree, "/option_tree",
            |s: &Engine, o: &mut Response| {
                o.write_i32(*s.settings.option_tree.as_ref().ok_or(ManualError::Absent)?)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                *s.settings.option_tree.as_mut().ok_or(ManualError::Absent)? =
                    parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            OptionTree2A, "/option_tree2/a",
            |s: &Engine, o: &mut Response| {
                o.write_i32(s.settings.option_tree2.as_ref().ok_or(ManualError::Absent)?.a)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.option_tree2.as_mut().ok_or(ManualError::Absent)?.a =
                    parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            OptionTree2B, "/option_tree2/b",
            |s: &Engine, o: &mut Response| {
                o.write_u8(s.settings.option_tree2.as_ref().ok_or(ManualError::Absent)?.b)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.option_tree2.as_mut().ok_or(ManualError::Absent)?.b =
                    parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayOptionTree0A, "/array_option_tree/0/a",
            |s: &Engine, o: &mut Response| {
                o.write_i32(s.settings.array_option_tree[0].as_ref().ok_or(ManualError::Absent)?.a)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.array_option_tree[0].as_mut().ok_or(ManualError::Absent)?.a =
                    parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayOptionTree0B, "/array_option_tree/0/b",
            |s: &Engine, o: &mut Response| {
                o.write_u8(s.settings.array_option_tree[0].as_ref().ok_or(ManualError::Absent)?.b)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.array_option_tree[0].as_mut().ok_or(ManualError::Absent)?.b =
                    parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayOptionTree1A, "/array_option_tree/1/a",
            |s: &Engine, o: &mut Response| {
                o.write_i32(s.settings.array_option_tree[1].as_ref().ok_or(ManualError::Absent)?.a)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.array_option_tree[1].as_mut().ok_or(ManualError::Absent)?.a =
                    parse_i32(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            ArrayOptionTree1B, "/array_option_tree/1/b",
            |s: &Engine, o: &mut Response| {
                o.write_u8(s.settings.array_option_tree[1].as_ref().ok_or(ManualError::Absent)?.b)
                    .map_err(ManualError::Codec)
            },
            |s: &mut Engine, i: &str| {
                s.settings.array_option_tree[1].as_mut().ok_or(ManualError::Absent)?.b =
                    parse_u8(i).map_err(ManualError::Codec)?;
                Ok(())
            };

            OptionArray, "/option_array",
            |s: &Engine, o: &mut Response| match s.settings.option_array {
                None => o.write_option_i32(None).map_err(ManualError::Codec),
                Some(_) => Err(ManualError::Unsupported),
            },
            |s: &mut Engine, i: &str| {
                if i == "null" {
                    s.settings.option_array = None;
                    Ok(())
                } else {
                    Err(ManualError::Unsupported)
                }
            };
        }
    };
}

macro_rules! define_key {
    ( $( $key:ident, $path:literal, $get:expr, $set:expr; )* ) => {
        #[derive(Copy, Clone)]
        enum Key {
            $( $key, )*
        }

        impl Key {
            fn parse(path: &str) -> Result<Self, ManualError> {
                match path {
                    $( $path => Ok(Self::$key), )*
                    _ => Err(ManualError::InvalidPath),
                }
            }
        }
    };
}

manual_leafs!(define_key);

macro_rules! impl_leaf_access {
    ( $( $key:ident, $path:literal, $get:expr, $set:expr; )* ) => {
        fn serialize_key(&self, key: Key, out: &mut Response) -> Result<(), ManualError> {
            out.clear();
            match key {
                $( Key::$key => ($get)(self, out), )*
            }
        }

        fn deserialize_key(&mut self, key: Key, input: &str) -> Result<(), ManualError> {
            match key {
                $( Key::$key => ($set)(self, input), )*
            }
        }
    };
}

impl Engine {
    fn enum_tree_get_c(&self, idx: usize, field_a: bool) -> Result<i32, ManualError> {
        match &self.settings.enum_tree {
            MyEnum::C(items) => {
                if field_a {
                    Ok(items[idx].a)
                } else {
                    Ok(items[idx].b as i32)
                }
            }
            _ => Err(ManualError::Absent),
        }
    }

    fn enum_tree_set_c_i32(&mut self, idx: usize, value: i32) -> Result<(), ManualError> {
        match &mut self.settings.enum_tree {
            MyEnum::C(items) => {
                items[idx].a = value;
                Ok(())
            }
            _ => Err(ManualError::Absent),
        }
    }

    fn enum_tree_set_c_u8(&mut self, idx: usize, value: u8) -> Result<(), ManualError> {
        match &mut self.settings.enum_tree {
            MyEnum::C(items) => {
                items[idx].b = value;
                Ok(())
            }
            _ => Err(ManualError::Absent),
        }
    }

    manual_leafs!(impl_leaf_access);
}

impl crate::Engine for Engine {
    type Error = ManualError;

    fn new() -> Self {
        Self {
            settings: Settings::new(),
        }
    }

    fn set(&mut self, path: &str, value: &str) -> Result<(), Self::Error> {
        self.deserialize_key(Key::parse(path)?, value)
    }

    fn get(&self, path: &str, out: &mut Response) -> Result<(), Self::Error> {
        self.serialize_key(Key::parse(path)?, out)
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}
