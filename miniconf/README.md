# miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/quartiq/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` exposes selected values inside heterogeneous Rust data as a small
tree. It is `no_std` by default, uses Serde for leaf payloads, and supports
runtime access by paths, compact keys, schema iteration, and metadata.

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

The common user-facing layers are:

- [`TreeSchema`]: static schema, exact lookup, leaf iteration, and metadata.
- [`TreeSerialize`]: serialize one selected leaf.
- [`TreeDeserialize`]: deserialize one selected leaf.
- [`TreeAny`]: access leaf values through `core::any::Any`.
- [`json_core`]: JSON helpers using slash-separated paths and `serde_json_core`.

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
    #[tree(rename = "cal")]
    #[tree(with = leaf)]
    calibration: Calibration,
}

let mut settings = Settings::default();
json_core::set(&mut settings, "/cal", br#"{"offset":-3,"scale":10}"#).unwrap();
assert_eq!(settings.calibration.offset, -3);
```

Structs, enums, arrays, tuples, `Option<T>`, and standard container types can be
combined into larger trees. `Option` branches and inactive enum variants remain
in the static schema but may return [`ValueError::Absent`] at runtime.

## Keys, Formats, And Transports

Public helper APIs accept [`IntoKeys`]. The default `&str` input is a rooted
slash path such as `/output/gain/1`. Use [`PathIter`], [`ConstPathIter`],
[`JsonPathIter`], index slices, or [`Packed`] when another key representation is
the better boundary format.

[`Path`], [`ConstPath`], [`JsonPath`], [`Indices`], and [`Packed`] can also be
used as schema iteration or transcode targets. [`NodeIter`] yields leaves only
and exposes its current indices and schema while walking.

`miniconf` is transport agnostic. Any channel that can carry key-value payloads
can use the tree. The core crate provides JSON helpers through [`json_core`] and
allocation-backed [`json`] helpers; [`postcard`] gives compact binary
serialization that pairs well with [`Packed`]. MQTT settings management lives in
the `miniconf_mqtt` crate.

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
