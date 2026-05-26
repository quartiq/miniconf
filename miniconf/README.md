# miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/quartiq/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` turns selected values inside heterogeneous Rust data into a small
runtime-addressable tree. It is `no_std` by default, uses Serde for leaf
payloads, and lets the same settings type serve human tools, compact embedded
links, generated schemas, and transport protocols.

Use it when a typed Rust configuration or state tree should be:

- accessed one leaf at a time by path or compact key
- exposed over a transport without giving that transport ownership of the data
- discovered by tools through schema iteration, semantics, and metadata
- reused across CLIs, SCPI-like protocols, MQTT, tests, or generated UI/API
  surfaces

## Quick Start

Derive [`Tree`] for the settings type. Fields whose types also implement the
`Tree*` traits become internal nodes; ordinary Serde values are leaves.

```rust
use miniconf::{json_core, Tree};

#[derive(Default, Tree)]
struct Settings {
    enabled: bool,
    output: Output,
}

#[derive(Default, Tree)]
struct Output {
    gain: [u16; 2],
}

let mut settings = Settings::default();

json_core::set(&mut settings, "/enabled", b"true").unwrap();
json_core::set(&mut settings, "/output/gain/1", b"42").unwrap();

let mut buf = [0; 8];
let len = json_core::get(&settings, "/output/gain/1", &mut buf).unwrap();

assert!(settings.enabled);
assert_eq!(&buf[..len], b"42");
```

## Pick The Surface

Start with [`json_core`] and slash-separated `&str` paths for human-facing
tools, tests, and protocol sketches. The lower layers are useful when the
boundary needs something more specific:

- [`TreeSchema`] and [`Schema::nodes()`] discover leaves; [`Schema::get()`]
  checks one exact key and returns the reached schema.
- [`TreeSerialize`] and [`TreeDeserialize`] serialize or update exactly one
  selected leaf with any Serde format.
- [`TreeAny`] gives typed host-side access through `core::any::Any`.
- [`PathIter`], [`ConstPathIter`], [`JsonPathIter`], index slices, and
  [`Packed`] are interchangeable key boundaries through [`IntoKeys`].
- [`postcard`] with [`Packed`] gives compact binary key-value messages.
- [`json_schema`] builds host/tooling schemas from the same tree.
- `miniconf_mqtt` is the ready-made MQTT transport.

## Tree Shape

`Tree` is a derive shorthand for [`macro@TreeSchema`], [`macro@TreeSerialize`],
[`macro@TreeDeserialize`], and [`macro@TreeAny`]. Derive attributes live under
`#[tree(...)]`:

- `rename = "name"` changes a field or variant path segment.
- `skip` removes a field or variant from the tree.
- `flatten` splices a single unambiguous child tree into its parent.
- `with = module` delegates access to a custom implementation module.
- `meta(...)` attaches schema metadata when the matching metadata feature is enabled.

Use `#[tree(with = leaf)]` to keep a type as one Serde leaf even if it also
implements `Tree`.

```rust
use miniconf::{json_core, leaf, Tree};
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct Calibration {
    offset: i32,
    scale: u16,
}

#[derive(Default, Tree)]
struct Settings {
    #[tree(rename = "cal", with = leaf)]
    calibration: Calibration,
}

let mut settings = Settings::default();
json_core::set(&mut settings, "/cal", br#"{"offset":-3,"scale":10}"#).unwrap();
assert_eq!(settings.calibration.offset, -3);
```

Structs, enums, arrays, tuples, `Option<T>`, and standard container types can be
combined into larger trees. `Option` branches and inactive enum variants remain
in the static schema but may return [`ValueError::Absent`] at runtime.

## Adapting Boundaries

`miniconf` is transport agnostic. Any channel that can carry a key and a Serde
payload can use the tree. Keep transport routing, sessions, and buffering in
the transport layer; pass a borrow of the settings tree into `miniconf` access
functions when a message targets the tree.

Use [`Schema::transcode()`] to translate one key representation into another.
Use [`NodeIter`] when publishing, validating, or rendering every leaf; it yields
leaves only and exposes the current indices and schema while walking.

## Limits

- Internal tree enums support unit, newtype, and skipped variants only. Enums
  with named fields or multi-field tuple variants should stay leaves or use a
  manual/custom implementation.
- Flattening is accepted only when generated lookup stays structurally
  unambiguous.
- `&str` key input is always slash-separated. Use explicit iterator types for
  other syntaxes or separators.
- Schema semantics and metadata are feature-gated reflection data. Do not depend
  on them unless `sem`, `meta-node`, or `meta-edge` is enabled as needed.

## Features

- `derive`: re-export derive macros from `miniconf_derive`; enabled by default.
- `json-core`: `serde_json_core` helpers for JSON byte slices.
- `json`: `serde_json` helpers.
- `postcard`: compact binary helpers using `postcard`.
- `sem`, `meta-node`, `meta-edge`: retain structured schema semantics, node
  metadata, and parent-child edge metadata. Constructors and derive output accept
  these payloads in all builds; without the matching feature, they are discarded
  and schema accessors return `None` or empty metadata.
- `trace`, `schema`: serde-reflection tracing and JSON Schema generation.
- `heapless`, `heapless-09`, `alloc`, `std`: support for the corresponding
  storage and platform layers.
