//! Caller-side codec erasure; the adapter itself needs neither alloc nor erased-serde.
use miniconf::{IntoKeys, Keys, Leaf, SerdeError, TreeDeserialize, TreeSerialize};
use serde::{Serialize, Serializer};

// A fresh cursor makes Serialize repeatable.
struct Selected<'a, T, K>(&'a T, K);

impl<T: TreeSerialize, K: Keys + Clone> Serialize for Selected<'_, T, K> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut keys = self.1.clone();
        self.0
            .serialize_by_key(&mut keys as &mut dyn Keys, serializer)
            .map_err(|error| match error {
                // Forward codec errors unchanged, including erased-serde's control signal.
                SerdeError::Inner(e) | SerdeError::Finalization(e) => e,
                SerdeError::Value(e) => serde::ser::Error::custom(e),
            })
    }
}

#[test]
fn serialize_repeatedly_across_codecs() {
    let tree = (7u32, Leaf([3u32, 4]));
    for (path, json, bytes) in [("/0", "7", &[7][..]), ("/1", "[3,4]", &[3, 4][..])] {
        let selected = Selected(&tree, path.into_keys());
        let value: &dyn erased_serde::Serialize = &selected;
        for _ in 0..2 {
            assert_eq!(serde_json::to_string(value).unwrap(), json);
            assert_eq!(
                serde_json::to_value(value).unwrap(),
                serde_json::from_str::<serde_json::Value>(json).unwrap()
            );
            assert_eq!(postcard::to_slice(value, &mut [0; 16]).unwrap(), bytes);
        }
        assert_eq!(
            postcard::to_slice(value, &mut []).unwrap_err(),
            postcard::Error::SerializeBufferFull
        );
    }
    let selected = Selected(&tree, "/missing".into_keys());
    let value: &dyn erased_serde::Serialize = &selected;
    assert!(
        serde_json::to_string(value)
            .unwrap_err()
            .to_string()
            .contains("Key not found")
    );
}

fn set<'de, T: TreeDeserialize<'de>>(
    tree: &mut T,
    keys: &mut dyn Keys,
    de: &mut dyn erased_serde::Deserializer<'de>,
) -> Result<(), SerdeError<erased_serde::Error>> {
    tree.deserialize_by_key(keys, de)
}

#[test]
fn deserialize_borrowed_values_and_finalize() {
    let mut tree = [""];
    let input = br#""borrowed" trailing"#;
    let mut json = serde_json::Deserializer::from_slice(input);
    set(
        &mut tree,
        &mut "/0".into_keys(),
        &mut <dyn erased_serde::Deserializer<'_>>::erase(&mut json),
    )
    .unwrap();
    assert_eq!(tree[0], "borrowed");
    assert_eq!(tree[0].as_ptr(), input[1..].as_ptr());
    // The update succeeds before the caller detects trailing data.
    assert!(json.end().is_err());

    let mut buffer = [0; 32];
    let input = postcard::to_slice("postcard", &mut buffer).unwrap();
    let mut de = postcard::Deserializer::from_bytes(input);
    set(
        &mut tree,
        &mut [0usize].as_slice(),
        &mut <dyn erased_serde::Deserializer<'_>>::erase(&mut de),
    )
    .unwrap();
    assert!(de.finalize().unwrap().is_empty());
    assert_eq!(tree[0], "postcard");
    assert_eq!(tree[0].as_ptr(), input[1..].as_ptr());
}
