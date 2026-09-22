# miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/quartiq/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` makes typed Rust data addressable: read a value, change it, discover
what else is there. Derive one tree and reuse it in a shell, a snapshot, an
inspector, or a protocol.

The core is `no_std` and needs no allocator. Serde encodes the values;
Miniconf selects them by path.

## Quick Start

Create a host executable (no hardware or network required):

```sh
cargo new miniconf-demo
cd miniconf-demo
cargo add miniconf@0.21.1
```

Replace `src/main.rs` with the following and run `cargo run`.
Derive [`Tree`] to expose the fields of each settings struct.

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
Try adding a field: it gets a path without another dispatch table. The final
loop discovers those paths from the type's schema. Here it uses the host's
`String`; a fixed-capacity string also works when no allocator is available.

## Tree Shape

Nested trees expose their fields separately, as `Output` does above. A leaf is
read or written as one Serde value. On the `gain` field,
`#[tree(with = miniconf::leaf)]` would make the array one leaf:
`/output/gain` would read or write `[0, 42]` as a whole instead of exposing each
element. `#[tree(rename = "level")]` would change its path segment to `level`.

Structs, enums, arrays, tuples, `Option<T>`, and standard container types can be
combined into larger trees. `Option` branches and inactive enum variants remain
in the static schema but may return [`ValueError::Absent`] at runtime.

## Control Changes

To reject invalid settings without changing the live tree, deserialize into a
candidate and commit only after the complete call succeeds. A failed call can
leave partial changes, including on payload finalization errors. Applying
hardware changes and saving settings remain application decisions.

Use `#[tree(with = module)]` to enforce rules for a field. The
[integration fixture](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/common.rs)
shows read-only fields and DAC range checking with a scratch copy. Metadata such
as `max = "4095"` describes the range; the custom deserializer enforces it.

## Reuse The Tree

Stabilizer's [miniconf-settings](https://github.com/quartiq/stabilizer/tree/292f6f3fa15b4a51789d97a08cbd7546ca3f3d06/miniconf-settings)
uses one tree for a shell and snapshots: `get` and `set` address live values,
while snapshots walk the leaves to save and restore them. Add a field and both
consumers can reach it. The application handles USB framing, hardware updates,
and flash storage. This upper-layer crate is currently unpublished.

The checkout examples explore other consumers using the same integration
fixture. Its paths and values are also used by tests and the embedded benchmark;
use the small quickstart tree above for experiments.

| Example | Run | What it adds |
| --- | --- | --- |
| [CLI](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/cli.rs) | `cargo run --example cli -- --output-dac-1 2048` | Command-line options |
| [Packed](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/packed.rs) | `cargo run --example packed --features postcard` | A binary leaf round trip in fixed buffers |
| [Schema](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/trace.rs) | `cargo run --example trace --features schema` | Host-side JSON and JSON Schema |
| [SCPI sketch](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/scpi.rs) | `cargo run --example scpi` | Custom command syntax, not a complete SCPI implementation |

## Build A Consumer

`Tree` derives four independent capabilities: [`TreeSchema`] describes the
leaves, [`TreeSerialize`] reads them, [`TreeDeserialize`] writes them, and
[`TreeAny`] borrows their values through `core::any::Any`. An inspector can
require only the first two. The caller supplies framing, buffers, and scheduling.

Paths are one way to select a leaf. Index slices and [`Packed`] keys use the same
[`IntoKeys`] interface; [`Schema::transcode()`] converts between representations.
Compact keys belong to a particular schema, so resolve them again when the tree
changes. [`Schema::nodes()`] discovers leaves and [`Schema::get()`] looks up one
key.

Choose the leaf codec independently: [`json_core`] uses JSON byte slices,
[`postcard`](https://docs.rs/miniconf/latest/miniconf/postcard/) uses compact
binary payloads. [`miniconf_mqtt`](https://docs.rs/miniconf_mqtt) and
[`miniconf_coap`](https://docs.rs/miniconf_coap) are ready-made protocol consumers.

## Code Size

The [embedded benchmark](https://github.com/quartiq/miniconf/tree/main/miniconf/tests/benchmark)
compares the same get/set workload and codec against handwritten dispatch.
It reports program size, schema bytes, and observed stack use. The manual
handler omits discovery and reflection; the results are workload-specific,
not a worst-case stack bound. Run it with your tree when size matters.

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

## Stability

`miniconf` follows [Cargo's SemVer compatibility guidelines][cargo-semver],
including [Rust's policy for trait implementations][trait-impls]. For `0.y.z`
releases, breaking changes bump `y`; compatible changes bump `z`.

[cargo-semver]: https://doc.rust-lang.org/cargo/reference/semver.html
[trait-impls]: https://rust-lang.github.io/rfcs/1105-api-evolution.html#trait-implementations
