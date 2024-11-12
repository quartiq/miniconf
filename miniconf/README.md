# miniconf: serialize/deserialize/access reflection for trees

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` enables lightweight (`no_std`/no alloc) serialization, deserialization,
and access within a tree of heretogeneous types by keys.

## Example

See below for an example showing some of the features of the `Tree*` traits.
See also the documentation and doctests of the [`TreeKey`] trait for a detailed description.

Note that the example below focuses on JSON and slash-separated paths while in fact
any `serde` backend (or `dyn Any` trait objects) and many different `Keys`/`Transcode`
providers are supported.

```rust
use serde::{Deserialize, Serialize};
use miniconf::{Error, json, JsonPath, Traversal, Tree, TreeKey, Path, Packed, Node, Leaf, Metadata};

#[derive(Deserialize, Serialize, Default, Tree)]
pub struct Inner {
    a: Leaf<i32>,
    b: Leaf<i32>,
}

#[derive(Deserialize, Serialize, Default, Tree)]
pub enum Either {
    #[default]
    Bad,
    Good,
    A(Leaf<i32>),
    B(Inner),
    C([Inner; 2]),
}

#[derive(Tree, Default)]
pub struct Settings {
    foo: Leaf<bool>,
    enum_: Leaf<Either>,
    struct_: Leaf<Inner>,
    array: Leaf<[i32; 2]>,
    option: Leaf<Option<i32>>,

    #[tree(skip)]
    #[allow(unused)]
    skipped: (),

    struct_tree: Inner,
    enum_tree: Either,
    array_tree: [Leaf<i32>; 2],
    array_tree2: [Inner; 2],
    tuple_tree: (Leaf<i32>, Inner),
    option_tree: Option<Leaf<i32>>,
    option_tree2: Option<Inner>,
    array_option_tree: [Option<Inner>; 2],
}

let mut settings = Settings::default();

// Access nodes by field name
json::set(&mut settings,"/foo", b"true")?;
assert_eq!(*settings.foo, true);
json::set(&mut settings, "/enum_", br#""Good""#)?;
json::set(&mut settings, "/struct_", br#"{"a": 3, "b": 3}"#)?;
json::set(&mut settings, "/array", b"[6, 6]")?;
json::set(&mut settings, "/option", b"12")?;
json::set(&mut settings, "/option", b"null")?;

// Nodes inside containers
// ... by field name in a struct
json::set(&mut settings, "/struct_tree/a", b"4")?;
// ... or by index in an array
json::set(&mut settings, "/array_tree/0", b"7")?;
// ... or by index and then struct field name
json::set(&mut settings, "/array_tree2/0/a", b"11")?;
// ... or by hierarchical index
json::set_by_key(&mut settings, [8, 0, 1], b"8")?;
// ... or by packed index
let (packed, node): (Packed, _) = Settings::transcode([8, 1, 0]).unwrap();
assert_eq!(packed.into_lsb().get(), 0b1_1000_1_0);
assert_eq!(node, Node::leaf(3));
json::set_by_key(&mut settings, packed, b"9")?;
// ... or by JSON path
json::set_by_key(&mut settings, &JsonPath(".array_tree2[1].b"), b"10")?;

// Hiding paths by setting an Option to `None` at runtime
assert_eq!(json::set(&mut settings, "/option_tree", b"13"), Err(Traversal::Absent(1).into()));
settings.option_tree = Some(0.into());
json::set(&mut settings, "/option_tree", b"13")?;
// Hiding a path and descending into the inner `Tree`
settings.option_tree2 = Some(Inner::default());
json::set(&mut settings, "/option_tree2/a", b"14")?;
// Hiding items of an array of `Tree`s
settings.array_option_tree[1] = Some(Inner::default());
json::set(&mut settings, "/array_option_tree/1/a", b"15")?;

let mut buf = [0; 16];

// Serializing nodes by path
let len = json::get(&settings, "/struct_", &mut buf).unwrap();
assert_eq!(&buf[..len], br#"{"a":3,"b":3}"#);

// Tree metadata
let meta: Metadata = Settings::traverse_all().unwrap();
assert!(meta.max_depth <= 6);
assert!(meta.max_length("/") <= 32);

// Iterating over all leaf paths
for path in Settings::nodes::<Path<heapless::String<32>, '/'>, 6>() {
    let (path, node) = path.unwrap();
    assert!(node.is_leaf());
    // Serialize each
    match json::get(&settings, &path, &mut buf) {
        // Full round-trip: deserialize and set again
        Ok(len) => { json::set(&mut settings, &path, &buf[..len])?; }
        // Some Options are `None`, some enum variants are absent
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
client in the `miniconf_mqtt` crate and a Python reference implementation to interact with it.
Miniconf is agnostic of the `serde` backend/format, key type/format, and transport/protocol.

## Formats

`miniconf` can be used with any `serde::Serializer`/`serde::Deserializer` backend, and key format.

Explicit support for `/` as the path hierarchy separator and JSON (`serde_json_core`) is implemented.

Support for the `postcard` wire format with any `postcard` flavor and
any [`Keys`] type is implemented. Combined with the [`Packed`] key representation, this is a very
space-efficient serde-by-key API.

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

For structs `miniconf` offers derive macros for [`macro@TreeKey`], [`macro@TreeSerialize`], [`macro@TreeDeserialize`], and [`macro@TreeAny`].
The macros implements the [`TreeKey`], [`TreeSerialize`], [`TreeDeserialize`], and [`TreeAny`] traits.
Fields/variants that form internal nodes (non-leaf) need to implement the respective `Tree{Key,Serialize,Deserialize,Any}` trait.
Leaf fields/items need to support the respective [`serde`] (and the desired `serde::Serializer`/`serde::Deserializer`
backend) or [`core::any`] trait.

Structs, enums, arrays, and Options can then be cascaded to construct more complex trees.

See also the [`TreeKey`] trait documentation for details.

## Keys and paths

Lookup into the tree is done using a [`Keys`] implementation. A blanket implementation through [`IntoKeys`]
is provided for `IntoIterator`s over [`Key`] items. The [`Key`] lookup capability is implemented
for `usize` indices and `&str` names.

Path iteration is supported with arbitrary separator `char`s between names.

Very compact hierarchical indices encodings can be obtained from the [`Packed`] structure.
It implements [`Keys`].

## Limitations

* `enum`: The derive macros don't support enums with record (named fields) variants or tuple variants with more than one field. Only unit, newtype and skipped variants are supported. Without the derive macros, these `enums` are still however usable in their atomic `serde` form as leaf nodes. Inline tuple variants are supported.
* The derive macros don't handle `std`/`alloc` smart pointers ( `Box`, `Rc`, `Arc`) in any special way. They however still be handled with accessors (`get`, `get_mut`, `validate`).
* The derive macros only support flattening in non-ambiguous situations (single field structs and single variant enums, both modulo skipped fields/variants and unit variants).

## Features

* `json-core`: Enable helper functions for serializing from and
  into json slices (using the `serde_json_core` crate).
* `postcard`: Enable helper functions for serializing from and
  into the postcard compact binary format (using the `postcard` crate).
* `derive`: Enable the derive macros in `miniconf_derive`. Enabled by default.

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
* ➕ Trait "reflection": Together with [`crosstrait`](https://crates.io/crates/crosstrait) supports building the
  type registry and enables casting from `dyn Any` returned by `TreeAny` to other desired trait objects.
  Together with [`erased-serde`](https://crates.io/crates/erased-serde) it can be used to implement node serialization/deserialization
  using `miniconf`'s `TreeAny` without using `TreeSerialize`/`TreeDeserialize` similar to `bevy_reflect`.

Some tangential crates:

* [`serde-reflection`](https://crates.io/crates/serde-reflection): extract schemata from serde impls
* [`typetag`](https://crates.io/crates/typetag): "derive serde for trait objects" (local traits and impls)
* [`deflect`](https://crates.io/crates/deflect): reflection on trait objects using adjacent DWARF debug info as the type registry
* [`intertrait`](https://crates.io/crates/intertrait): inspiration and source of ideas for `crosstrait`

## Functional Programming, Polymorphism

The type-heterogeneity of `miniconf` also borders on functional programming features. For that crates like the
following may also be relevant:

* [`frunk`](https://crates.io/crates/frunk)
* [`lens-rs`](https://crates.io/crates/lens-rs)/[`rovv`](https://crates.io/crates/rovv)
