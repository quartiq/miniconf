//! Utilities using `serde_json`
use alloc::{
    string::{String, ToString},
    vec,
};
use serde_json::value::{Serializer as ValueSerializer, Value};

use crate::{Internal, IntoKeys, KeyError, Schema, SerdeError, TreeSerialize, ValueError};

/// Magic JSON Value for absent node values.
pub const TREE_ABSENT: &str = "__tree-absent__";
/// Magic JSON Value for access-denied node values.
pub const TREE_ACCESS: &str = "__tree-access__";

/// Serialize a TreeSerialize into a JSON Value
pub fn to_json_value<T: TreeSerialize>(
    value: &T,
) -> Result<Value, SerdeError<<ValueSerializer as serde::Serializer>::Error>> {
    enum NodeValue<T> {
        Value(T),
        Absent,
        Access,
    }

    impl<T: Into<Value>> NodeValue<T> {
        fn into_value(self) -> Value {
            match self {
                Self::Value(value) => value.into(),
                Self::Absent => Value::String(TREE_ABSENT.to_string()),
                Self::Access => Value::String(TREE_ACCESS.to_string()),
            }
        }
    }

    fn classify<E>(
        result: Result<Value, SerdeError<E>>,
    ) -> Result<NodeValue<Value>, SerdeError<E>> {
        match result {
            Ok(value) => Ok(NodeValue::Value(value)),
            Err(SerdeError::Value(ValueError::Absent)) => Ok(NodeValue::Absent),
            Err(SerdeError::Value(ValueError::Access(_msg))) => Ok(NodeValue::Access),
            Err(err) => Err(err),
        }
    }

    fn insert_named(
        object: &mut serde_json::Map<String, Value>,
        name: &str,
        value: NodeValue<Value>,
    ) {
        if let NodeValue::Absent = value {
            return;
        }
        object.insert(name.to_string(), value.into_value());
    }

    fn visit<T: TreeSerialize>(
        idx: &mut [usize],
        depth: usize,
        schema: &Schema,
        value: &T,
    ) -> Result<NodeValue<Value>, SerdeError<<ValueSerializer as serde::Serializer>::Error>> {
        match classify(value.serialize_by_key((&idx[..depth]).into_keys(), ValueSerializer)) {
            Ok(NodeValue::Value(value)) => Ok(NodeValue::Value(value)),
            Ok(NodeValue::Absent) => Ok(NodeValue::Absent),
            Ok(NodeValue::Access) => Ok(NodeValue::Access),
            Err(SerdeError::Value(ValueError::Key(KeyError::TooShort))) => {
                let Some(internal) = schema.internal() else {
                    unreachable!("TooShort implies an internal schema");
                };
                Ok(NodeValue::Value(match internal {
                    Internal::Homogeneous(h) => Value::Array(
                        (0..h.len.get())
                            .map(|i| {
                                idx[depth] = i;
                                visit(idx, depth + 1, h.schema, value).map(NodeValue::into_value)
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                    Internal::Named(n) => {
                        let mut object = serde_json::Map::with_capacity(n.len());
                        for (i, n) in n.iter().enumerate() {
                            idx[depth] = i;
                            insert_named(
                                &mut object,
                                n.name,
                                visit(idx, depth + 1, n.schema, value)?,
                            );
                        }
                        Value::Object(object)
                    }
                    Internal::Numbered(n) => Value::Array(
                        n.iter()
                            .enumerate()
                            .map(|(i, n)| {
                                idx[depth] = i;
                                visit(idx, depth + 1, n.schema, value).map(NodeValue::into_value)
                            })
                            .collect::<Result<_, _>>()?,
                    ),
                }))
            }
            Err(err) => Err(err),
        }
    }
    Ok(visit(
        &mut vec![0; T::SCHEMA.shape().max_depth],
        0,
        T::SCHEMA,
        value,
    )?
    .into_value())
}
