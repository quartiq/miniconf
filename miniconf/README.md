# miniconf

[![crates.io](https://img.shields.io/crates/v/miniconf.svg)](https://crates.io/crates/miniconf)
[![docs](https://docs.rs/miniconf/badge.svg)](https://docs.rs/miniconf)
[![QUARTIQ Matrix Chat](https://img.shields.io/matrix/quartiq:matrix.org)](https://matrix.to/#/#quartiq:matrix.org)
[![Continuous Integration](https://github.com/quartiq/miniconf/workflows/Continuous%20Integration/badge.svg)](https://github.com/quartiq/miniconf/actions)

`miniconf` makes typed Rust data addressable: read a leaf, change it, discover
what else is there. Derive one tree and reuse it in a shell, a snapshot, an
inspector, or a protocol.

The core is `no_std` and needs no allocator. Serde encodes the leaf values;
Miniconf supplies paths, compact keys, and discovery. For whole-value
serialization alone, Serde is enough.

## Quick Start

Create a host executable (no hardware or network required):

```sh
cargo new miniconf-demo
cd miniconf-demo
cargo add miniconf@0.21.1
```

Replace `src/main.rs` with the following and run `cargo run`.
Derive [`Tree`] for the settings type. Fields whose types also implement the
`Tree*` traits become internal nodes; ordinary Serde values are leaves.

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
Try adding a field: it gets a path without another dispatch table.
For embedded use, replace the host's `String` path storage with a fixed-capacity
buffer or [`Indices`]/[`Packed`] keys.

## One Tree, Several Consumers

From a checkout, these independent examples use the same
[settings type](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/common.rs):

| Example | Run | Boundary |
| --- | --- | --- |
| [CLI](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/cli.rs) | `cargo run --example cli -- --output-dac-1 2048` | Command-line options to leaf access |
| [Packed](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/packed.rs) | `cargo run --example packed --features postcard` | Compact keys and binary leaf payloads |
| [Schema](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/trace.rs) | `cargo run --example trace --features schema` | Host-side JSON and JSON Schema generation |

For custom key syntax, see the [SCPI sketch](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/scpi.rs)
(`cargo run --example scpi`; not a complete SCPI implementation).

[Add a field. Keep the interfaces.](https://github.com/quartiq/miniconf/blob/main/miniconf/INTEGRATION.md)
follows the same idea through Stabilizer's shell and snapshots.

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
- [`postcard`](https://docs.rs/miniconf/latest/miniconf/postcard/) with [`Packed`]
  gives compact binary key-value access.
- [`json_schema`](https://docs.rs/miniconf/latest/miniconf/json_schema/) builds
  host/tooling schemas from the same tree.
- [`miniconf_mqtt`](https://docs.rs/miniconf_mqtt) and
  [`miniconf_coap`](https://docs.rs/miniconf_coap) provide optional protocol layers.

## Tree Shape

`Tree` is a derive shorthand for [`macro@TreeSchema`], [`macro@TreeSerialize`],
[`macro@TreeDeserialize`], and [`macro@TreeAny`]. Derive attributes live under
`#[tree(...)]`:

- `rename = ident` changes a field or variant path segment to a Rust identifier.
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

Bring a key, a payload, and a borrow of the tree. The caller owns framing,
buffers, scheduling, and application effects. A read-only inspector can require
only `TreeSchema` and `TreeSerialize`.

[`Schema::transcode()`] translates keys; [`NodeIter`] walks leaves. Compact
keys belong to a particular schema, so resolve them again when the tree changes.
Discovery includes branches that may be absent or unwritable at runtime.

### Validation And Application Effects

The [shared example](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/common.rs)
uses `#[tree(with = module)]` for read-only fields and DAC range checking.
Its `dac` module checks a scratch copy before replacing the array.
Metadata such as `max = "4095"` is descriptive; the custom deserializer enforces
the range.

A failed deserialization can leave partial changes, including on payload
finalization errors. For request-level rollback, validate a candidate and commit
only after the complete call succeeds. Applying hardware changes and saving
settings remain application decisions.

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
