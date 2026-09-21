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

Serde handles each leaf's value; Miniconf locates that leaf and describes the
tree around it. Use Serde alone when reading or writing a whole value is enough.
A small fixed interface may only need a handwritten match. Miniconf is useful
when the same tree needs selective access, discovery, or several consumers.

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

This prints `/enabled`, `/output/gain/0`, and `/output/gain/1`. Adding a field
to the derived tree makes it addressable and discoverable without another
dispatch table. The host example uses `String` to print paths; embedded callers
can use fixed-capacity path buffers or [`Indices`] and [`Packed`] keys.

## One Tree, Several Consumers

From a checkout, these independent examples use the same
[settings type](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/common.rs):

| Example | Run | Boundary |
| --- | --- | --- |
| [CLI](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/cli.rs) | `cargo run --example cli -- --output-dac-1 2048` | Command-line options to leaf access |
| [Packed](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/packed.rs) | `cargo run --example packed --features postcard` | Compact keys and binary leaf payloads |
| [Schema](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/trace.rs) | `cargo run --example trace --features schema` | Host-side JSON and JSON Schema generation |

The [SCPI sketch](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/scpi.rs)
shows a custom key syntax (`cargo run --example scpi`); it is not a complete
SCPI implementation. Shells, persistence, inspectors, and transport adapters
can each build on the core without owning the other layers.

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

`miniconf` is transport agnostic. Any channel that can carry a key and a Serde
payload can use the tree. Keep transport routing, sessions, and buffering in
the transport layer; pass a borrow of the settings tree into `miniconf` access
functions when a message targets the tree.

Use [`Schema::transcode()`] to translate one key representation into another.
Use [`NodeIter`] when publishing, validating, or rendering every leaf; it yields
leaves only and exposes the current indices and schema while walking.

An upper layer normally needs only three operations:

| Task | Core operation | Caller responsibility |
| --- | --- | --- |
| Read or write one leaf | `json_core::{get,set}` or the `TreeSerialize`/`TreeDeserialize` traits | Frame the request, provide buffers, report errors |
| List available paths | `Settings::SCHEMA.nodes()` | Choose key storage and traversal depth; handle absent live branches |
| Reuse a resolved key | `Settings::SCHEMA.transcode()` | Keep the key paired with the schema it was resolved against |

The core does not require an executor, socket, or lock. Borrow the tree for the
operation, then let the application decide when to apply settings to hardware.
An upper layer can use only the traits it needs: a read-only inspector need not
require `TreeDeserialize`. Compact keys describe positions in a particular
schema; they are not persistent identifiers across arbitrary tree changes.

### Validation And Application Effects

The [shared example](https://github.com/quartiq/miniconf/blob/main/miniconf/examples/common.rs)
shows two small access policies through `#[tree(with = module)]`:

- `read_only` denies deserialization while retaining serialization and discovery.
- `dac` deserializes into a scratch copy, checks the 12-bit range, and replaces
  the selected array only after validation succeeds.

Metadata such as `max = "4095"` describes a value; it does not enforce its range.
The custom deserializer enforces that invariant. Likewise, discovering a path
does not guarantee that its value is present or writable at runtime.

Deserialization is not generally transactional: an error can follow a partial
mutation, including an error while finalizing a payload. If the whole request
must leave the live tree unchanged on failure, deserialize and validate a
candidate, then commit it after the complete operation succeeds. Choose the
smallest candidate that covers the application's invariant. Multi-leaf updates,
hardware effects, and persistence need their own application-level commit policy.

Keep settings operations distinct from their transport. Stabilizer's
[`miniconf-settings`](https://github.com/quartiq/stabilizer/tree/main/miniconf-settings)
composes a shell, snapshots, and flash storage above Miniconf; UART/USB ownership
and the timing of application effects remain outside that crate. It is currently
an unpublished workspace crate, not an additional requirement for using Miniconf.

## Code Size

The embedded benchmark compares `miniconf` against a handwritten serial-style
router for the same settings tree and value codec. Treat the handwritten router
as a routed get/set lower bound, not a feature-equivalent replacement: it omits
schema iteration, metadata, key transcoding, generic key backends, and generated
reflection. The benchmark reports the static schema payload separately so the
routed get/set overhead and reflection data can be judged independently.

See the [benchmark instructions and results](https://github.com/quartiq/miniconf/tree/main/miniconf/tests/benchmark)
for the target, feature set, build profile, and reproduction command. These are
whole-program sizes and an observed stack high-water mark for one workload,
not a universal per-setting cost or a worst-case stack bound.

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
