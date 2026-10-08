# miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/quartiq/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` makes typed Rust data addressable: read a value, change it, discover
what else is there. Derive one tree and reuse it in a shell, a snapshot, an
inspector, or a protocol.

The core is `no_std` and needs no allocator. Serde encodes the values;
Miniconf selects them.

## Quick Start

```rust
use miniconf::{json_core, ConstPath, Tree, TreeSchema};

#[derive(Default, Tree)]
struct Settings {
    enabled: bool,
    output: Output,
}

#[derive(Default, Tree)]
struct Output {
    gain: [u16; 2],
}

fn main() {
    let mut settings = Settings::default();

    json_core::set(&mut settings, "/enabled", b"true").unwrap();
    json_core::set(&mut settings, "/output/gain/1", b"42").unwrap();

    let mut buf = [0; 8];
    let len = json_core::get(&settings, "/output/gain/1", &mut buf).unwrap();

    assert!(settings.enabled);
    assert_eq!(&buf[..len], b"42");

    const DEPTH: usize = Settings::SCHEMA.max_depth();
    for path in Settings::SCHEMA.nodes::<ConstPath<String, '/'>, DEPTH>() {
        println!("{}", path.unwrap());
    }
}
```

This prints `/enabled`, `/output/gain/0`, and `/output/gain/1`.
Add a field and it gets a path automatically. The final loop discovers the
available paths without constructing another dispatch table.

Paths are empty for the root or start with `/`.

## Tree Shape

Nested trees expose their fields separately, as `Output` does above. A leaf is
read or written as one Serde value. On the `gain` field,
`#[tree(with = miniconf::leaf)]` would make the array one leaf:
`/output/gain` would read or write `[0, 42]` as a whole instead of exposing each
element. `#[tree(rename = "level")]` would change its path segment to `level`.

Structs, enums, arrays, tuples, `Option<T>`, and standard container types can be
combined into larger trees. `Option` branches and inactive enum variants remain
in the static schema but may return [`ValueError::Absent`] at runtime.

Use `#[tree(with = module)]` for validation, read-only fields, or application
hooks. `examples/common.rs` demonstrates read-only values and range checking.
Metadata describes constraints; it does not enforce them.

Internal enums support unit and newtype variants; other variants can be skipped.
Keep enums with named or multi-field variants as Serde leaves.

## Reuse The Tree

Try the same settings tree through different interfaces:

| Run | What it adds |
| --- | --- |
| `cargo run --example cli -- --output-dac-1 2048` | Command-line options |
| `cargo run --example packed --features postcard` | A binary leaf round trip in fixed buffers |
| `cargo run --example trace --features schema` | Host-side JSON and JSON Schema |
| `cargo run --example scpi` | A small SCPI-style command interface |

[`miniconf_mqtt`](https://docs.rs/miniconf_mqtt) and
[`miniconf_coap`](https://docs.rs/miniconf_coap) expose trees over MQTT and CoAP.

The code-size benchmark in `tests/benchmark` compares get/set against
handwritten dispatch with the same codec.

## Build A Consumer

`Tree` derives four independent capabilities: [`TreeSchema`] describes the
structure, [`TreeSerialize`] reads leaves, [`TreeDeserialize`] writes them, and
[`TreeAny`] borrows their values through `core::any::Any`. An inspector can
require only the first two. The caller supplies framing, buffers, and scheduling.

Paths are one way to select a leaf. Index slices and [`Packed`] keys use the same
[`IntoKeys`] interface; [`Schema::transcode()`] converts between representations.
[`Schema::nodes()`] enumerates leaves; [`Schema::get()`] looks up any node.

Choose the leaf codec independently: [`json_core`] uses JSON byte slices,
[`postcard`](https://docs.rs/miniconf/latest/miniconf/postcard/) uses compact
binary payloads, and custom consumers can supply any Serde format.

## Features

Defaults enable `derive`, `json-core`, `sem`, `meta-node`, `meta-edge`, and
`heapless-09`. Disable default features to select only what you need.

- `derive`: derive the tree traits.
- `json-core`: `serde_json_core` helpers for JSON byte slices.
- `json`: `serde_json` helpers.
- `postcard`: compact binary helpers using `postcard`.
- `sem`, `meta-node`, `meta-edge`: schema semantics, node and edge metadata.
- `trace`, `schema`: serde-reflection tracing and JSON Schema generation.
- `heapless`, `heapless-09`, `alloc`, `std`: support for the corresponding
  storage and platform layers.
- `defmt`: embedded diagnostics formatting.

## Stability

`miniconf` follows [Cargo's SemVer compatibility guidelines](https://doc.rust-lang.org/cargo/reference/semver.html)
including [Rust's policy for trait implementations](https://rust-lang.github.io/rfcs/1105-api-evolution.html#trait-implementations).
For `0.y.z` releases, breaking changes bump `y`; compatible changes bump `z`.
