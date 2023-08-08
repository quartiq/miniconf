# Miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

Miniconf enables lightweight (`no_std`) partial serialization (retrieval) and deserialization
(updates, modification) within a tree by key. The tree is backed by
structs/arrays/Options of serializable types.

Miniconf can be used as a very simple and flexible backend for run-time settings management in embedded devices
over any transport. It was originally designed to work with JSON ([serde_json_core](https://docs.rs/serde-json-core))
payloads over MQTT ([minimq](https://docs.rs/minimq)) and provides a comlete [MQTT settings management
client](MqttClient) and a Python reference implementation to ineract with it.
`Miniconf` is completely generic over the `serde::Serializer`/`serde::Deserializer` backend and the path hierarchy separator.

## Example

```rust
use miniconf::{Error, JsonCoreSlash, Tree, TreeKey};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Copy, Clone, Default)]
enum Either {
    #[default]
    Bad,
    Good,
}

#[derive(Deserialize, Serialize, Copy, Clone, Default, Tree)]
struct Inner {
    a: i32,
    b: i32,
}

#[derive(Tree, Default)]
struct Settings {
    // Atomic updtes by field name
    foo: bool,
    enum_: Either,
    struct_: Inner,
    array: [i32; 2],
    option: Option<i32>,

    // Exposing elements of containers
    // ... by field name
    #[tree()]
    struct_defer: Inner,
    // ... or by index
    #[tree()]
    array_defer: [i32; 2],
    // ... or deferring to two levels (index and then inner field name)
    #[tree(depth(2))]
    array_miniconf: [Inner; 2],

    // Hiding paths by setting the Option to `None` at runtime
    #[tree()]
    option_defer: Option<i32>,
    // Hiding a path and deferring to the inner
    #[tree(depth(2))]
    option_miniconf: Option<Inner>,
    // Hiding elements of an Array of Tree items
    #[tree(depth(3))]
    array_option_miniconf: [Option<Inner>; 2],
}

let mut settings = Settings::default();
let mut buf = [0; 64];

// Atomic updates by field name
settings.set_json("/foo", b"true")?;
assert_eq!(settings.foo, true);
settings.set_json("/enum_", br#""Good""#)?;
settings.set_json("/struct_", br#"{"a": 3, "b": 3}"#)?;
settings.set_json("/array", b"[6, 6]")?;
settings.set_json("/option", b"12")?;
settings.set_json("/option", b"null")?;

// Deep access by field name in a struct
settings.set_json("/struct_defer/a", b"4")?;
// ... or by index in an array
settings.set_json("/array_defer/0", b"7")?;
// ... or by index and then struct field name
settings.set_json("/array_miniconf/1/b", b"11")?;

// If a deferred Option is `None` it is hidden at runtime and can't be accessed
settings.option_defer = None;
assert_eq!(settings.set_json("/option_defer", b"13"), Err(Error::Absent(1)));
settings.option_defer = Some(0);
settings.set_json("/option_defer", b"13")?;
settings.option_miniconf = Some(Inner::default());
settings.set_json("/option_miniconf/a", b"14")?;
settings.array_option_miniconf[1] = Some(Inner::default());
settings.set_json("/array_option_miniconf/1/a", b"15")?;

// Serializing elements by path
let len = settings.get_json("/struct_", &mut buf)?;
assert_eq!(&buf[..len], br#"{"a":3,"b":3}"#);

// Iterating over all paths
for path in Settings::iter_paths::<String>("/") {
    let path = path.unwrap();
    // Serializing each
    match settings.get_json(&path, &mut buf) {
        Ok(len) => {
            // Deserialize again
            settings.set_json(&path, &buf[..len])?;
        }
        // Some settings are still `None` and thus their paths are expected to be absent
        Err(Error::Absent(_)) => {}
        e => {
            e?;
        }
    }
}

# Ok::<(), miniconf::Error<serde_json_core::de::Error>>(())
```

## MQTT

There is an [MQTT-based client](MqttClient) that implements settings management over the [MQTT
protocol](https://mqtt.org) with JSON payloads. A Python reference library is provided that
interfaces with it.

```sh
# Discover the complete unique prefix of an application listening to messages
# under the topic `quartiq/application/12345` and set its `foo` setting to `true`.
python -m miniconf -d quartiq/application/+ foo=true
```

## Derive macro

For structs Miniconf offers derive macros for [`macro@TreeKey`], [`macro@TreeSerialize`], and [`macro@TreeDeserialize`].
The macros implements the [`TreeKey`], [`TreeSerialize`], and [`TreeDeserialize`] traits that exposes access to serialized field values through their path.
Fields that form internal nodes (non-leaf) need to implement the respective `Tree{Key,Serialize,Deserialize}` trait. Fields that are
leafs need to support the respective [serde] trait (and the `serde::Serializer`/`serde::Deserializer` backend).

Structs, arrays, and Options can then be cascaded to construct more complex trees.
When using the derive macro, the behavior and tree recursion depth can be configured for each
struct field using the `#[tree(depth(Y))]` attribute.

See also the [`Tree`] trait documentation for details.

## Keys and paths

Lookup into the tree is done using an iterator over [Key] items. `usize` indices or `&str` names are supported.

Path iteration is supported with arbitrary separator between names.

## Formats

Miniconf is generic over the `serde` backend/payload format and the path hierarchy separator.

Currently support for `/` as the path hierarchy separator and JSON (`serde_json_core`) is implemented
through the [JsonCoreSlash] super trait.

## Transport

Miniconf is designed to be protocol-agnostic. Any means that can receive key-value input from
some external source can be used to modify values by path.

## Limitations

Deferred/deep/non-atomic access to inner elements of some types is not yet supported, e.g. enums
other than [core::option::Option]. These are still however usable in their atomic `serde` form as leaves.

## Features

* `mqtt-client` Enable the MQTT client feature. See the example in [MqttClient].
* `json-core` Enable the [JsonCoreSlash] implementation of serializing from and
  into json slices (using `serde_json_core`).

The `mqtt-client` and `json-core` features are enabled by default.
