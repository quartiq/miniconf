#![cfg(all(feature = "derive", feature = "json-core"))]

use miniconf::{IntoKeys, Leaf, SerdeError, TreeDeserialize, json_core};

#[test]
fn recovering_deserializer_cannot_swallow_finalization() {
    #[derive(Debug, PartialEq)]
    struct Recover(u8);
    impl<'de> serde::Deserialize<'de> for Recover {
        fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            Ok(Self(u8::deserialize(de).unwrap_or(0)))
        }
    }
    let mut value = Leaf(Recover(1));
    assert!(matches!(
        json_core::set(&mut value, "", b"9 trailing"),
        Err(SerdeError::Finalization(_))
    ));
    assert_eq!(value.0, Recover(1));
}

#[path = "../examples/common.rs"]
mod common;

#[test]
fn seed_uses_field_context_before_assignment() {
    #[derive(miniconf::TreeSchema, TreeDeserialize)]
    struct Settings {
        #[tree(with = limited, typ = "u8")]
        level: (u8, u8),
    }

    mod limited {
        use miniconf::{Keys, SerdeError, TreeDeserializer};
        use serde::{
            Deserialize, Deserializer,
            de::{DeserializeSeed, Error},
        };

        pub use miniconf::leaf::{probe_by_key, schema};

        struct Limit(u8);
        struct Checked(u8);

        impl<'de> DeserializeSeed<'de> for Limit {
            type Value = Checked;

            fn deserialize<D: Deserializer<'de>>(self, de: D) -> Result<Checked, D::Error> {
                let value = u8::deserialize(de)?;
                if value > self.0 {
                    return Err(D::Error::custom("level exceeds limit"));
                }
                Ok(Checked(value))
            }
        }

        pub fn deserialize_by_key<'de, D: TreeDeserializer<'de>>(
            value: &mut (u8, u8),
            mut keys: impl Keys,
            de: D,
        ) -> Result<D::Ok, SerdeError<D::Error>> {
            keys.finalize()?;
            let (next, output) = de.deserialize_seed(Limit(value.1))?;
            value.0 = next.0;
            Ok(output)
        }
    }

    let mut settings = Settings { level: (1, 10) };
    assert!(matches!(
        json_core::set(&mut settings, "/level", b"11"),
        Err(SerdeError::Inner(_))
    ));
    assert_eq!(settings.level, (1, 10));
    assert!(matches!(
        json_core::set(&mut settings, "/level", b"9 trailing"),
        Err(SerdeError::Finalization(_))
    ));
    assert_eq!(settings.level, (1, 10));
    assert_eq!(json_core::set(&mut settings, "/level", b"9"), Ok(1));
    assert_eq!(settings.level, (9, 10));
    let mut raw = serde_json_core::de::Deserializer::new(b"8", None);
    settings
        .deserialize_by_key("/level".into_keys(), &mut raw)
        .unwrap();
    assert_eq!(settings.level, (8, 10));
    settings.level.1 = 7;
    assert!(json_core::set(&mut settings, "/level", b"8").is_err());
    assert_eq!(settings.level, (8, 7));
}

#[test]
fn json_rejects_before_assignment() {
    let mut settings = common::Settings::new();
    for (path, data) in [
        ("/control/enabled", b"false trailing".as_slice()),
        ("/control/mode", br#""Standby" trailing"#),
        ("/output/dac/0", b"7 trailing"),
        ("/calibration/offset", b"9 trailing"),
    ] {
        let before = settings.clone();
        assert!(matches!(
            json_core::set(&mut settings, path, data),
            Err(SerdeError::Finalization(_))
        ));
        assert!(settings == before);
    }
    let before = settings.clone();
    assert!(matches!(
        json_core::set(&mut settings, "/output/dac/0", b"5000"),
        Err(SerdeError::Value(_))
    ));
    assert!(settings == before);
    assert_eq!(
        json_core::set(&mut settings, "/control/enabled", b"false \n"),
        Ok(7)
    );
    assert!(!settings.control.enabled);
}

#[test]
fn composite_leaf_errors_preserve_value() {
    let mut value = Leaf([1u16, 2]);
    assert!(matches!(
        json_core::set(&mut value, "", b"[9]"),
        Err(SerdeError::Inner(_))
    ));
    assert_eq!(value.0, [1, 2]);
    assert!(matches!(
        json_core::set(&mut value, "", b"[9,8] trailing"),
        Err(SerdeError::Finalization(_))
    ));
    assert_eq!(value.0, [1, 2]);
    json_core::set(&mut value, "", b"[9,8]").unwrap();
    assert_eq!(value.0, [9, 8]);

    // Raw sources retain Serde's in-place behavior, including partial updates on errors.
    let mut de = serde_json_core::de::Deserializer::new(b"[7]", None);
    assert!(value.deserialize_by_key("".into_keys(), &mut de).is_err());
    assert_eq!(value.0, [7, 8]);
}

#[test]
fn raw_source_reuses_leaf_storage() {
    let mut value = Leaf(Vec::<u16>::with_capacity(8));
    value.0.extend([1, 2, 3]);
    let storage = value.0.as_ptr();
    let mut de = serde_json_core::de::Deserializer::new(b"[9,8]", None);
    value.deserialize_by_key("".into_keys(), &mut de).unwrap();
    assert_eq!(value.0, [9, 8]);
    assert_eq!(value.0.as_ptr(), storage);
    assert_eq!(value.0.capacity(), 8);
}

#[test]
fn borrowed_and_nested_values() {
    let mut value = Leaf(Some(("old", [1u8, 2])));
    assert!(json_core::set(&mut value, "", br#"["new",[3,4]] trailing"#).is_err());
    assert_eq!(value.0, Some(("old", [1, 2])));
    let input = br#"["new",[3,4]]"#;
    json_core::set(&mut value, "", input).unwrap();
    let (text, array) = value.0.unwrap();
    assert_eq!(text, "new");
    assert_eq!(text.as_ptr(), input[2..].as_ptr());
    assert_eq!(array, [3, 4]);
}

#[cfg(feature = "heapless-09")]
#[test]
fn bounded_collection_and_struct() {
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Record {
        values: heapless_09::Vec<u16, 2>,
    }
    let mut value = Leaf(Record {
        values: heapless_09::Vec::from_slice(&[1]).unwrap(),
    });
    for input in [
        br#"{"values":[2,3,4]}"#.as_slice(),
        br#"{"values":[2]} trailing"#,
    ] {
        assert!(json_core::set(&mut value, "", input).is_err());
        assert_eq!(value.0.values.as_slice(), &[1]);
    }
    json_core::set(&mut value, "", br#"{"values":[2,3]}"#).unwrap();
    assert_eq!(value.0.values.as_slice(), &[2, 3]);
}

#[cfg(feature = "postcard")]
#[test]
fn postcard_finalization_and_remainder() {
    use postcard::{
        Error,
        de_flavors::{Flavor, Slice},
    };

    struct Reject<'de>(Slice<'de>);
    impl<'de> Flavor<'de> for Reject<'de> {
        type Source = &'de [u8];
        type Remainder = &'de [u8];
        fn pop(&mut self) -> Result<u8, Error> {
            self.0.pop()
        }
        fn try_take_n(&mut self, ct: usize) -> Result<&'de [u8], Error> {
            self.0.try_take_n(ct)
        }
        fn finalize(self) -> Result<Self::Remainder, Error> {
            Err(Error::DeserializeBadCrc)
        }
    }

    let mut value = Leaf([1u16, 2]);
    assert!(matches!(
        miniconf::postcard::set_by_key(&mut value, "", Reject(Slice::new(&[9, 8]))),
        Err(SerdeError::Finalization(Error::DeserializeBadCrc))
    ));
    assert_eq!(value.0, [1, 2]);
    assert!(matches!(
        miniconf::postcard::set_by_key(&mut value, "", Slice::new(&[9])),
        Err(SerdeError::Inner(_))
    ));
    assert_eq!(value.0, [1, 2]);
    let remaining = miniconf::postcard::set_by_key(&mut value, "", Slice::new(&[9, 8, 7])).unwrap();
    assert_eq!(value.0, [9, 8]);
    assert_eq!(remaining, [7]);
}
