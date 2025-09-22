//! Showcase for reflection and schema building

use serde_json::{Value, value::Serializer};

use miniconf::{
    Internal, IntoKeys, KeyError, Schema, SerdeError, TreeSerialize, ValueError,
    json_schema::TreeJsonSchema,
};

mod common;
use common::Settings;

fn to_json_value<T: TreeSerialize>(
    value: &T,
) -> Result<Value, SerdeError<<Serializer as serde::Serializer>::Error>> {
    fn visit<T: TreeSerialize>(
        idx: &mut [usize],
        depth: usize,
        schema: &Schema,
        value: &T,
    ) -> Result<Value, SerdeError<<Serializer as serde::Serializer>::Error>> {
        match value.serialize_by_key((&idx[..depth]).into_keys(), Serializer) {
            Ok(v) => Ok(v),
            Err(SerdeError::Value(ValueError::Absent)) => {
                Ok(Value::String("__tree-absent__".to_string()))
            }
            Err(SerdeError::Value(ValueError::Access(_msg))) => {
                Ok(Value::String("__tree-access__".to_string()))
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
                    Internal::Named(n) => Value::Object(
                        n.iter()
                            .enumerate()
                            .map(|(i, n)| {
                                idx[depth] = i;
                                Ok((n.name.to_string(), visit(idx, depth + 1, n.schema, value)?))
                            })
                            .collect::<Result<_, SerdeError<_>>>()?,
                    ),
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

fn main() -> anyhow::Result<()> {
    let s = Settings::new();

    let value = to_json_value(&s)?;
    println!("JSON Tree:\n{}", serde_json::to_string_pretty(&value)?);

    let mut schema = TreeJsonSchema::new(Some(&s)).unwrap();

    schema
        .root
        .insert("title".to_string(), "Miniconf example: Settings".into());

    use schemars::transform::Transform;
    //miniconf::json_schema::Strictify.transform(&mut schema.root);
    miniconf::json_schema::AllowAbsent.transform(&mut schema.root);

    println!(
        "JSON Schema:\n{}",
        serde_json::to_string_pretty(&schema.root)?
    );

    jsonschema::meta::validate(schema.root.as_value()).unwrap();

    let validator = jsonschema::validator_for(schema.root.as_value())?;
    for e in validator.iter_errors(&value) {
        eprintln!("{e} {e:?}");
    }

    Ok(())
}
