# Miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`Miniconf` enables lightweight (`no_std`) partial serialization (retrieval) and deserialization
(updates, modification) within a tree by key. The tree is backed by
structs/arrays/Options of serializable types.

## Example

See below for a comprehensive example showing the features of the `Tree` traits.
See also the documentation of the [`TreeKey`] trait for a detailed description.

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

    // Exclude an element (not Deserialize/Serialize)
    #[tree(skip)]
    skipped: (),

    // Exposing elements of containers
    // ... by field name
    #[tree(depth=1)]
    struct_tree: Inner,
    // ... or by index
    #[tree(depth=1)]
    array_tree: [i32; 2],
    // ... or exposing two levels (array index and then inner field name)
    #[tree(depth=2)]
    array_tree2: [Inner; 2],

    // Hiding paths by setting the Option to `None` at runtime
    #[tree(depth=1)]
    option_tree: Option<i32>,
    // Hiding a path and descending into the inner `Tree`
    #[tree(depth=2)]
    option_tree2: Option<Inner>,
    // Hiding elements of an array of `Tree`s
    #[tree(depth=3)]
    array_option_tree: [Option<Inner>; 2],
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
settings.set_json("/struct_tree/a", b"4")?;
// ... or by index in an array
settings.set_json("/array_tree/0", b"7")?;
// ... or by index and then struct field name
settings.set_json("/array_tree2/1/b", b"11")?;

// If a `Tree`-Option is `None` it is hidden at runtime and can't be serialized/deserialized.
settings.option_tree = None;
assert_eq!(settings.set_json("/option_tree", b"13"), Err(Error::Absent(1)));
settings.option_tree = Some(0);
settings.set_json("/option_tree", b"13")?;
settings.option_tree2 = Some(Inner::default());
settings.set_json("/option_tree2/a", b"14")?;
settings.array_option_tree[1] = Some(Inner::default());
settings.set_json("/array_option_tree/1/a", b"15")?;

// Serializing elements by path
let len = settings.get_json("/struct_", &mut buf).unwrap();
assert_eq!(&buf[..len], br#"{"a":3,"b":3}"#);

// Iterating over all paths
for path in Settings::iter_paths::<String>("/") {
    let path = path.unwrap();
    // Serializing each
    match settings.get_json(&path, &mut buf) {
        // Full round-trip: deserialize and set again
        Ok(len) => { settings.set_json(&path, &buf[..len])?; }
        // Some settings are still `None` and thus their paths are expected to be absent
        Err(Error::Absent(_)) => {}
        e => { e.unwrap(); }
    }
}

# Ok::<(), Error<serde_json_core::de::Error>>(())
```

## Settings management

One possible use of `Miniconf` is a backend for run-time settings management in embedded devices.

It was originally designed to work with JSON ([`serde_json_core`](https://docs.rs/serde-json-core))
payloads over MQTT ([`minimq`](https://docs.rs/minimq)) and provides a MQTT settings management
client and a Python reference implementation to interact with it. Now it is agnostic of
`serde` backend/format, hierarchy separator, and transport/protocol.

## Formats

`Miniconf` can be used with any `serde::Serializer`/`serde::Deserializer` backend, and key format.

Currently support for `/` as the path hierarchy separator and JSON (`serde_json_core`) is implemented
through the [`JsonCoreSlash`] super trait.

The [`Postcard`] super trait supports the `postcard` wire format with any `postcard` flavor and
any [`Keys`] type. Combined with the [`Packed`] key representation, this is a very
space-efficient key-serde API.

Blanket implementations are provided for all
`TreeSerialize`+`TreeDeserialize` types for all formats.

## Transport

Miniconf is also protocol-agnostic. Any means that can receive or emit serialized key-value data
can be used to access nodes by path.

The `MqttClient` in the `miniconf_mqtt` crate implements settings management over the [MQTT
protocol](https://mqtt.org) with JSON payloads. A Python reference library is provided that
interfaces with it. This example discovers the unique prefix of an application listening to messages
under the topic `quartiq/application/12345` and set its `/foo` setting to `true`.

```sh
python -m miniconf -d quartiq/application/+ /foo=true
```

## Derive macros

For structs Miniconf offers derive macros for [`macro@TreeKey`], [`macro@TreeSerialize`], and [`macro@TreeDeserialize`].
The macros implements the [`TreeKey`], [`TreeSerialize`], and [`TreeDeserialize`] traits.
Fields/items that form internal nodes (non-leaf) need to implement the respective `Tree{Key,Serialize,Deserialize}` trait.
Leaf fields/items need to support the respective [`serde`] trait (and the desired `serde::Serializer`/`serde::Deserializer`
backend).

Structs, arrays, and Options can then be cascaded to construct more complex trees.
When using the derive macro, the behavior and tree recursion depth can be configured for each
struct field using the `#[tree(depth(Y))]` attribute.

See also the [`TreeKey`] trait documentation for details.

## Keys and paths

Lookup into the tree is done using a [`Keys`] implementation. A blanket implementation through [`IntoKeys`]
is provided for `IntoIterator`s over [`Key`] items. The [`Key`] lookup capability is implemented
for `usize` indices and `&str` names.

Path iteration is supported with arbitrary separator between names.

Very compact hierarchical indices encodings can be obtained from the [`Packed`] structure.
It implements [`Keys`].

## Limitations

Deferred/deep/non-atomic access to inner elements of some types is not yet supported, e.g. enums
other than [`Option`]. These are still however usable in their atomic `serde` form as leaves.

## Features

* `json-core`: Enable the [`JsonCoreSlash`] implementation of serializing from and
  into json slices (using `serde_json_core`).

The `json-core` feature is enabled by default.
