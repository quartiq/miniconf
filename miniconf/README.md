# miniconf: serialize/deserialize/access reflection for trees

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` enables lightweight (`no_std`/no alloc) serialization, deserialization,
and access within a tree of heretogeneous types by keys.

## Reflection

`miniconf` enables certain kinds of reflective access to heterogeneous trees.
Let's compare it to [`bevy_reflect`](https://crates.io/crates/bevy_reflect)
which is a comprehensive and mature reflection crate:

`bevy_reflect` is thoroughly `std` while `miniconf` aims at `no_std`.
`bevy_reflect` uses its `Reflect` trait to operate on and pass nodes as trait objects.
`miniconf` uses serialized data or `Any` to access leaf nodes and pure "code" to traverse through internal nodes.
The `Tree*` traits like `Reflect` thus give access to nodes but unlike `Reflect` they are all decidedly not object-safe
and can not be used as trait objects. This allows `miniconf` to support non-`'static` borrowed data
(only for `TreeAny` the leaf nodes need to be `'static`)
while `bevy_reflect` requires `'static` for `Reflect` types.

`miniconf`supports at least the following reflection features mentioned in the `bevy_reflect` README:

* ➕ Derive the traits: `miniconf` has `Tree*` derive macros and blanket implementations for arrays and Options.
  Leaf nodes just need some impls of `Serialize/Deserialize/Any` where desired.
* ➕ Interact with fields using their names
* ➖ "Patch" your types with new values: `miniconf` only supports limited changes to the tree structure at runtime
  (`Option` and custom accessors) while `bevy_reflect` has powerful dynamic typing tools.
* ➕ Look up nested fields using "path strings": In addition to a superset of JSON path style
  "path strings" `miniconf` supports hierarchical indices and bit-packed ordered keys.
* ➕ Iterate over struct fields: `miniconf` Supports recursive iteration over node keys.
* ➕ Automatically serialize and deserialize via Serde without explicit serde impls:
  `miniconf` supports automatic serializing/deserializing into key-value pairs without an explicit container serde impls.
* (➕) Trait "reflection": `miniconf` has no integrated support but the `std` crate [`intertrait`](https://crates.io/crates/intertrait)
  can be used to implement the type registry and cast from `dyn Any` returned by `TreeAny` to desired trait objects.
  Together with `erased-serde` it can be used to implement node serialization/deserialization
  using `miniconf`'s `TreeAny` without using `TreeSerialize`/`TreeDeserialize` similar to `bevy_reflect`, see the `reflect` example.
  Another interesting crate is [`deflect`](https://crates.io/crates/deflect)
  which allows reflection on trait objects (like `Any`) using adjacent DWARF debug info as the type registry.
  It's `std` and experimental.

Some tangential crates:

* [`serde-reflection`](https://crates.io/crates/serde-reflection): extract schemata
* [`typetag`](https://crates.io/crates/typetag): "derive serde for trait objects" (local traits and impls)

## Example

See below for an example showing some of the features of the `Tree*` traits.
See also the documentation of the [`TreeKey`] trait for a detailed description.

```rust
use serde::{Deserialize, Serialize};
use miniconf::{Error, JsonCoreSlash, JsonPath, Traversal, Tree, TreeKey, Packed};

#[derive(Deserialize, Serialize, Default)]
enum Either {
    #[default]
    Bad,
    Good,
}

#[derive(Deserialize, Serialize, Default, Tree)]
struct Inner {
    a: i32,
    b: i32,
}

#[derive(Tree, Default)]
struct Settings {
    foo: bool,
    enum_: Either,
    struct_: Inner,
    array: [i32; 2],
    option: Option<i32>,

    // Skipping (`()` is not Deserialize/Serialize)
    #[tree(skip)]
    skipped: (),

    #[tree(depth=1)]
    struct_tree: Inner,
    #[tree(depth=1)]
    array_tree: [i32; 2],
    #[tree(depth=2)]
    array_tree2: [Inner; 2],

    #[tree(depth=1)]
    option_tree: Option<i32>,
    #[tree(depth=2)]
    option_tree2: Option<Inner>,
    #[tree(depth=3)]
    array_option_tree: [Option<Inner>; 2],
}

let mut settings = Settings::default();

// Atomic updates by field name
settings.set_json("/foo", b"true")?;
assert_eq!(settings.foo, true);
settings.set_json("/enum_", br#""Good""#)?;
settings.set_json("/struct_", br#"{"a": 3, "b": 3}"#)?;
settings.set_json("/array", b"[6, 6]")?;
settings.set_json("/option", b"12")?;
settings.set_json("/option", b"null")?;

// Exposing nodes of containers
// ... by field name in a struct
settings.set_json("/struct_tree/a", b"4")?;
// ... or by index in an array
settings.set_json("/array_tree/0", b"7")?;
// ... or by index and then struct field name
settings.set_json("/array_tree2/0/a", b"11")?;
// ... or by hierarchical index
settings.set_json_by_key([7, 0, 1].into_iter(), b"8")?;
// ... or by packed index
let (packed, _depth) = Settings::packed([7, 1, 0]).unwrap();
assert_eq!(packed.into_lsb().get(), 0b1_0111_1_0);
settings.set_json_by_key(packed, b"9")?;
// ... or by JSON path
settings.set_json_by_key(JsonPath::from(".array_tree2[1].b"), b"10")?;

// Hiding paths by setting an Option to `None` at runtime
assert_eq!(settings.set_json("/option_tree", b"13"), Err(Traversal::Absent(1).into()));
settings.option_tree = Some(0);
settings.set_json("/option_tree", b"13")?;
// Hiding a path and descending into the inner `Tree`
settings.option_tree2 = Some(Inner::default());
settings.set_json("/option_tree2/a", b"14")?;
// Hiding items of an array of `Tree`s
settings.array_option_tree[1] = Some(Inner::default());
settings.set_json("/array_option_tree/1/a", b"15")?;

let mut buf = [0; 16];

// Serializing nodes by path
let len = settings.get_json("/struct_", &mut buf).unwrap();
assert_eq!(&buf[..len], br#"{"a":3,"b":3}"#);

// Iterating over all paths
for path in Settings::iter_paths::<String>("/") {
    let path = path.unwrap();
    // Serialize each
    match settings.get_json(&path, &mut buf) {
        // Full round-trip: deserialize and set again
        Ok(len) => { settings.set_json(&path, &buf[..len])?; }
        // Some settings are still `None` and thus their paths are expected to be absent
        Err(Error::Traversal(Traversal::Absent(_))) => {}
        e => { e.unwrap(); }
    }
}

# Ok::<(), Error<serde_json_core::de::Error>>(())
```

## Settings management

One possible use of `miniconf` is a backend for run-time settings management in embedded devices.

It was originally designed to work with JSON ([`serde_json_core`](https://docs.rs/serde-json-core))
payloads over MQTT ([`minimq`](https://docs.rs/minimq)) and provides a MQTT settings management
client and a Python reference implementation to interact with it. Now it is agnostic of
`serde` backend/format, hierarchy separator, and transport/protocol.

## Formats

`miniconf` can be used with any `serde::Serializer`/`serde::Deserializer` backend, and key format.

Currently support for `/` as the path hierarchy separator and JSON (`serde_json_core`) is implemented
through the [`JsonCoreSlash`] super trait.

The `Postcard` super trait supports the `postcard` wire format with any `postcard` flavor and
any [`Keys`] type. Combined with the [`Packed`] key representation, this is a very
space-efficient key-serde API.

Blanket implementations are provided for all
`TreeSerialize`+`TreeDeserialize` types for all formats.

## Transport

`miniconf` is also protocol-agnostic. Any means that can receive or emit serialized key-value data
can be used to access nodes by path.

The `MqttClient` in the `miniconf_mqtt` crate implements settings management over the [MQTT
protocol](https://mqtt.org) with JSON payloads. A Python reference library is provided that
interfaces with it. This example discovers the unique prefix of an application listening to messages
under the topic `quartiq/application/12345` and set its `/foo` setting to `true`.

```sh
python -m miniconf -d quartiq/application/+ /foo=true
```

## Derive macros

For structs `miniconf` offers derive macros for [`macro@TreeKey`], [`macro@TreeSerialize`], and [`macro@TreeDeserialize`].
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

Access to inner fields of some types is not yet supported, e.g. enums
other than [`Option`]. These are still however usable in their atomic `serde` form as leaf nodes.

Many `std` smart pointers are not supported or handled in any special way: `Box`, `Rc`, `Arc`.

## Features

* `json-core`: Enable the [`JsonCoreSlash`] implementation of serializing from and
  into json slices (using the `serde_json_core` crate).
* `postcard`: Enable the `Postcard` implementation of serializing from and
  into the postcard compact binary format (using the `postcard` crate).
* `derive`: Enable the derive macros in `miniconf_derive`. Enabled by default.
