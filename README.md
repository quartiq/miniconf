# Miniconf
[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/vertigo-designs/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

Miniconf enables lightweight (`no_std`) partial serialization (retrieval) and deserialization
(updates, modification) within a hierarchical namespace by path. The namespace is backed by
structs and arrays of serializable types.

Miniconf can be used as a very simple and flexible backend for run-time settings management in embedded devices
over any transport. It was originally designed to work with JSON ([serde_json_core](https://docs.rs/serde-json-core))
payloads over MQTT ([minimq](https://docs.rs/minimq)) and provides a comlete [MQTT settings management
client](MqttClient) and a Python reference implementation to ineract with it.

## Example
```rust
use miniconf::{Error, Miniconf, MiniconfJson};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Copy, Clone, Default)]
enum Either {
    #[default]
    Bad,
    Good,
}

#[derive(Deserialize, Serialize, Copy, Clone, Default, Miniconf)]
struct Inner {
    a: i32,
    b: i32,
}

#[derive(Miniconf, Default)]
struct Settings {
    // Atomic updtes by field name
    foo: bool,
    enum_: Either,
    struct_: Inner,
    array: [i32; 2],
    option: Option<i32>,

    // Exposing elements of containers
    // ... by field name
    #[miniconf(defer)]
    struct_defer: Inner,
    // ... or by index
    #[miniconf(defer)]
    array_defer: [i32; 2],
    // ... or deferring to two levels (index and then inner field name)
    #[miniconf(defer)]
    array_miniconf: miniconf::Array<Inner, 2>,

    // Hiding paths by setting the Option to `None` at runtime
    #[miniconf(defer)]
    option_defer: Option<i32>,
    // Hiding a path and deferring to the inner
    #[miniconf(defer)]
    option_miniconf: miniconf::Option<Inner>,
    // Hiding elements of an Array of Miniconf items
    #[miniconf(defer)]
    array_option_miniconf: miniconf::Array<miniconf::Option<Inner>, 2>,
}

let mut settings = Settings::default();
let mut buf = [0; 64];

// Atomic updates by field name
settings.set("foo", b"true")?;
assert_eq!(settings.foo, true);
settings.set("enum_", br#""Good""#)?;
settings.set("struct_", br#"{"a": 3, "b": 3}"#)?;
settings.set("array", b"[6, 6]")?;
settings.set("option", b"12")?;
settings.set("option", b"null")?;

// Deep access by field name in a struct
settings.set("struct_defer/a", b"4")?;
// ... or by index in an array
settings.set("array_defer/0", b"7")?;
// ... or by index and then struct field name
settings.set("array_miniconf/1/b", b"11")?;

// If a deferred Option is `None` it is hidden at runtime and can't be accessed
settings.option_defer = None;
assert_eq!(settings.set("option_defer", b"13"), Err(Error::PathAbsent));
settings.option_defer = Some(0);
settings.set("option_defer", b"13")?;
settings.option_miniconf = Some(Inner::default()).into();
settings.set("option_miniconf/a", b"14")?;
settings.array_option_miniconf[1] = Some(Inner::default()).into();
settings.set("array_option_miniconf/1/a", b"15")?;

// Serializing elements by path
let len = settings.get("struct_", &mut buf)?;
assert_eq!(&buf[..len], br#"{"a":3,"b":3}"#);

// Iterating over and serializing all paths
for path in Settings::iter_paths::<3, 32>().unwrap() {
    let ret = settings.get(&path, &mut buf);

    // Some settings are still `None` and thus their paths are expected to be absent
    assert!(matches!(ret, Ok(_) | Err(Error::PathAbsent)));
}

# Ok::<(), miniconf::Error>(())
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

## Design
For structs with named fields, Miniconf offers a [derive macro](derive.Miniconf.html) to automatically
assign a unique path to each item in the namespace of the struct.
The macro implements the [Miniconf] trait that exposes access to serialized field values through their path.
All types supported by [serde_json_core] can be used as fields.

Elements of homogeneous [core::array]s are similarly accessed through their numeric indices.
Structs, arrays, and Options can then be cascaded to construct a multi-level namespace.
Namespace depth and access to individual elements instead of the atomic updates
is configured at compile (derive) time using the `#[miniconf(defer)]` attribute.
`Option` is used with `#[miniconf(defer)]` to support paths that may be absent (masked) at
runtime.

While the [Miniconf] implementations for [core::array] and [core::option::Option] by provide
atomic access to their respective inner element(s), [Array] and
[Option] have alternative [Miniconf] implementations that expose deep access
into the inner element(s) through their respective inner [Miniconf] implementations.

## Formats
The path hierarchy separator is the slash `/`.

Values are serialized into and deserialized from JSON.

## Transport
Miniconf is designed to be protocol-agnostic. Any means that can receive key-value input from
some external source can be used to modify values by path.

## Limitations
Deferred (non-atomic) access to inner elements of some types is not yet supported. This includes:
* Complex enums (other than [core::option::Option])
* Tuple structs (other than [Option], [Array])

## Features
* `mqtt-client` Enabled the MQTT client feature. See the example in [MqttClient].
