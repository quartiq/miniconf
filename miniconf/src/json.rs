//! Utilities using `serde_json`
use serde_json::value::{Serializer as ValueSerializer, Value};

use crate::{
    Internal, IntoKeys, KeyError, Schema, SerdeError, TreeSerialize, ValueError,
    json_schema::{TREE_ABSENT, TREE_ACCESS},
};

/// Serialize a TreeSerialize into a JSON Value
pub fn to_json_value<T: TreeSerialize>(
    value: &T,
) -> Result<Value, SerdeError<<ValueSerializer as serde::Serializer>::Error>> {
    fn visit<T: TreeSerialize>(
        idx: &mut [usize],
        depth: usize,
        schema: &Schema,
        value: &T,
    ) -> Result<Value, SerdeError<<ValueSerializer as serde::Serializer>::Error>> {
        match value.serialize_by_key((&idx[..depth]).into_keys(), ValueSerializer) {
            Ok(v) => Ok(v),
            Err(SerdeError::Value(ValueError::Absent)) => {
                Ok(Value::String(TREE_ABSENT.to_string()))
            }
            Err(SerdeError::Value(ValueError::Access(_msg))) => {
                Ok(Value::String(TREE_ACCESS.to_string()))
            }
            Err(SerdeError::Value(ValueError::Key(KeyError::TooShort))) => {
                Ok(match schema.internal.as_ref().unwrap() {
                    Internal::Homogeneous(h) => Value::Array(
                        (0..h.len.get())
                            .map(|i| {
                                idx[depth] = i;
                                visit(idx, depth + 1, h.schema, value)
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                    Internal::Named(n) => {
                        let mut object = serde_json::Map::with_capacity(n.len());
                        for (i, n) in n.iter().enumerate() {
                            idx[depth] = i;
                            let value = visit(idx, depth + 1, n.schema, value)?;
                            if matches!(&value, Value::String(absent) if absent == TREE_ABSENT) {
                                continue;
                            }
                            object.insert(n.name.to_string(), value);
                        }
                        Value::Object(object)
                    }
                    Internal::Numbered(n) => Value::Array(
                        n.iter()
                            .enumerate()
                            .map(|(i, n)| {
                                idx[depth] = i;
                                visit(idx, depth + 1, n.schema, value)
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                })
            }
            Err(err) => Err(err),
        }
    }
    visit(
        &mut vec![0; T::SCHEMA.shape().max_depth],
        0,
        T::SCHEMA,
        value,
    )
}
