<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.19.0](https://github.com/quartiq/miniconf/compare/v0.18.0...v0.19.0) - 2025-05-20

### Added

* py: a synchronous client version in `miniconf.sync`
* py: support for response-less (fire and forget) requests in both the synchronous and the asyncio client
* py: cli support for simple relative paths
* examples/trace: Tracing reflection with serde-reflection
* `impl Transcode for Vec<T: impl TryFrom<usize>>`

### Changed

* py: `await discover_one(...)` -> `one(await discover(...))`
* `serialize_by_key`: returns serializer `Ok` instead of depth
* `deserialize_by_key`: returns `()` instead of depth
* `TreeDeserialize`: add `probe_by_key()` for tracing without sample
* `Walk::internal()` takes slice of children by value
* `TreeKey::traverse_all()` and `Walk::internal()` are infallible
* derive: `validate`, `get`, `get_mut` replaced with `with(serialize=expr, deserialize=expr...)`

### Removed

* `Traversal::Invalid`: use `Traversal::Access`

## [0.18.0](https://github.com/quartiq/miniconf/compare/v0.17.2...v0.18.0) - 2024-11-22

### Changed

* `KeyLookup`, `Walk`, and `Metadata` layout and behavior

## [0.17.2](https://github.com/quartiq/miniconf/compare/v0.17.1...v0.17.2) - 2024-11-19

### Added

* `deny` proc macro field attribute for fine grained rejection of struct access
  (and removed corresponding trait impls).

## [0.17.1](https://github.com/quartiq/miniconf/compare/v0.17.0...v0.17.1) - 2024-11-12

### Added

* `std` and `alloc` features and `Tree*` impls for `Box`, `Rc`, `RcWeak`, `Arc`, `ArcWeak`,
  `Cow`, `Mutex`, `RwLock`, `Cell`, `RefCell`, `Bound`, `Range`, `RangeFrom`, `RangeTo`,
  `RangeInflusive`

## [0.17.0](https://github.com/quartiq/miniconf/compare/v0.16.3...v0.17.0) - 2024-10-25

### Changed

* The "recursion depth" const generic of the `Tree*` traits has been removed in favor of
  explicit newtypes implementing the traits for leaf values `Leaf<T>` and `StrLeaf<T>`.
  This reduces code duplication, macro usage and complexity.
  It enables any recursion depth, does away with most manual tracking of recursion depth
  in proc macro attributes, and simplifies code and derive macros, at the expense of having
  to wrap leaves in newtypes and having to pass an indices length to `TreeKey::nodes()`.
* `TreeKey::nodes` requires the indices length as a const generic.
* `get`, `get_mut`, `validate` proc macro attributes are now `Expr`
* `Key::find` and `Keys::finalize` return a `Result`, not an `Option` to reduce code duplication
* Derive macro lifetime and type param trait bound heuristics have been improved.
  They should now yield the correct result in mpst cases.
* Internal nodes must always have at least one leaf. Trait impls for `[T; 0]` and `()`
  have been removed. The `len` argument to the `traverse_by_key` closure is now a
  `NonZero<usize>`.

### Added

* `Leaf` to explicitly manage `Serialize/Deserialize` leaf values.
* `StrLeaf` to manage values via `AsRef<str>`/`TryFrom<&str>` (e.g. Enums via `strum`)
* `Tree*` impls for heterogeneous inline tuples `(T0, T1, ...)` up to length 8 (also useful
  for enum variants)
* `impl Tree* for &{mut,} T where T: Tree*` blanket impls to simplify usage downstream
* `defer` derive attribute to quickly defer to a downstream field without having to write accessors
* `Metadata` now also computes maximum `Packed` bits usage

### Removed

* `TreeSerialize`/`TreeDeserialize`/`TreeAny` don't need `TreeKey`

## [0.16.3](https://github.com/quartiq/miniconf/compare/v0.16.2...v0.16.3) - 2024-10-20

### Added

* `Transcode` slices of any integer type

## [0.16.2](https://github.com/quartiq/miniconf/compare/v0.16.1...v0.16.2) - 2024-10-17

### Added

* `Indices`/`Path` usability improvements avoiding needless borrowing

## [0.16.1](https://github.com/quartiq/miniconf/compare/v0.16.0...v0.16.1) - 2024-10-15

### Added

* `impl Key for &T where T: Key`

## [0.16.0](https://github.com/quartiq/miniconf/compare/v0.15.0...v0.16.0) - 2024-10-08

### Changed

* `KeyLookup` has been changed from a trait to a struct.
* `TreeKey::metadata() -> Metadata` has been changed to `traverse_all<W: Walk>() -> W` with `impl Walk for Metadata`.

### Added

* `Key` impls for `u8, u16, u32`.
* A `SCPI` exampling showcasing how to implement a custom `Key`.

## [0.15.0](https://github.com/quartiq/miniconf/compare/v0.14.1...v0.15.0) - 2024-10-01

### Changed

* `Traversal` and `Error` are now `exhaustive`.

### Removed

* The `JsonCoreSlash`/`JsonCoreSlashOwned` and `Postcard`/`PostcardOwned` traits have
  been removed in favor of simple functions in the `json::` and `postcard::` modules.

## [0.14.1](https://github.com/quartiq/miniconf/compare/v0.13.0...v0.14.1) - 2024-09-30

### Added

* `flatten` support for structs/enums with a single non-skip/non-unit variant/field.
* `core::error::Error` implementations added to `Error` and `Traversal`

## [0.14.0](https://github.com/quartiq/miniconf/compare/v0.13.0...v0.14.0) - 2024-09-26

### Added

* Derive support for enums with newtype/unit/skipped variants

### Removed

* The `KeyLookup::NAMES` associated constant is now an implementation
  detail

## [0.13.0](https://github.com/quartiq/miniconf/compare/v0.12.0...v0.13.0) - 2024-07-10

### Changed

* [miniconf_mqtt] the `/alive` message is now configurable
* [py/miniconf-mqtt] `discover()` returns the prefix and the alive payload
* [miniconf_mqtt] the prefix `&str` must now outlive the miniconf client

### Added

* [py/miniconf-mqtt]: `discover_one()`

## [0.12.0](https://github.com/quartiq/miniconf/compare/v0.11.0...v0.12.0) - 2024-07-09

### Changed

* `{Path,Indices,Packed}Iter` -> `NodeIter`
* `TreeKey::iter_{paths,indices,packed}` -> `TreeKey::nodes`
* `TreeKey::{path,indices,packed,json_path}` -> `TreeKey::transcode`/`Transcode::transcode`
* `crosstrait` now has its [own repository](https://github.com/quartiq/crosstrait)
* `Keys::is_empty()` -> `Keys::finalize()`
* `traverse_by_key` ensures `Keys::finalize()`
* `NodeIter::count()` -> `NodeIter::exact_size()` to disambiguate from `Iterator::count()`
* [miniconf_mqtt] path listing are done by publishing an empty payload to an internal node path
  with a response topic (no `/list` topic anymore)
* [py/miniconf-mqtt] The `Miniconf::create` method is no longer used. Instead, an `aiomqtt::Client`
  must be passed to Miniconf
* [py/miniconf-mqtt] `--list` option removed in favor of `PATH?` command

### Added

* Node iteration now supports limiting the iteration to a sub-tree by setting the iteration `root()`.
* `Transcode` trait for Keys transcoding and node lookup.
* `Transcode` and `NodeIter` now return `Node` with `NodeType` information (leaf or internal).
* `Keys::chain` and `Chain` to concatenate two `Keys` of different type.
* `miniconf_cli`: a menu/command line interface
* `Path`, `JsonPath`/`JsonPathIter`, `Indices`, `KeysIter` wrapper types for more ergonomic/succinct
  `Transcode`/`IntoKeys`/`Keys` handling
* [miniconf_mqtt] support on-demand and partial tree dump/list by posting
  the empty payload to an internal node without/with a response topic
* [py/miniconf-mqtt] support partial list (`PATH?`) and partial on-demand dump (`PATH!`)

### Removed

* `digits()` gone in favor of using `usize::checked_ilog10()`
* `rust_version` and `MSRV`: these crates aim to support the latest stable version of rust

## [0.11.0](https://github.com/quartiq/miniconf/compare/v0.10.1...v0.11.0) - 2024-04-30

### Changed

* [breaking] The `Traversal` error enum has been split from the `Error<E>` enum to reduce genericism.
* [breaking] `Increment` trait and blanket impl removed in favor of `increment_result`/
  `Error::increment`/`Traversal::increment`
* Uncounted iteration is the default
* [breaking] The `traverse_by_key` callback receives the field name as an `Option<&'static str>`
  (`None` in the case of arrays and tuple structs).
* [breaking] Derive macro attributes: accessor/validation revamp: `getter -> get`, `setter -> get_mut`,
  and `validate` with more idiomatic and flexible usage and call sequencing.
* [breaking] `Metadata.separator()` has been changed to only return the new maximum length for
  consistency and renamed to `max_length(separator: &str)`.
* [breaking] The `KeyLookup` trait has been split from `TreeKey`.
* [breaking] `minimq v0.9` requiring `embedded-nal v0.8`.

### Added

* `TreeAny` to access nodes trough `Any` trait objects.
* `TreeKey::json_path()` for JSON path notation `.bar[5]`
* `JsonPath: Keys`
* `rename` field attribute for derive macros
* Counted iteration is supported (includung `ExactSizeIterator`) through the `count()`
  "augmentation" methods on the iterators.
* `derive` feature in `miniconf` crate to control pulling in the derive macros, default enabled
* Limited depth iteration has been added.

### Removed

* [breaking] `TreeKey::iter_*_unchecked()` have been removed. Uncounted iteration is the default.

## [0.10.1](https://github.com/quartiq/miniconf/compare/v0.10.0...v0.10.1) - 2024-04-22

### Changed

* README changes to fix docs

## [0.10.0](https://github.com/quartiq/miniconf/compare/v0.9.0...v0.10.0) - 2024-04-22

### Changed

* [breaking] Python lib signatures have changed (Miniconf.create(), discover())
* Python lib discovery timeout has been optimized to work well for both slow
  connections (high RTT) and fast ones
* [breaking] The MQTT client does not own the miniconf settings struct anymore.
* [breaking] `handled_update()` has been removed from the MQTT client in favor of validator/getter/setter callbacks.
* [breaking] The MQTT client has been split into its own `miniconf_mqtt` crate.
* [breaking] The attribute syntax has changed from `#[tree(depth(1))]` to `#[tree(depth=1)]`.
* [breaking] The default depth is `0`, also in the case where a `#[tree()]` without `depth` has been specified.
* [breaking] The `traverse_by_key` callback also receives the number of indices at the given level.
* The trait methods are now generic over `Keys` and not over `Iterator<Item: Key>`.
  A blanket implementation has been provided.
* `JsonCoreSlash::{set,get}_json_by_indices()` removed in favor of `{get,set}_json_by_key()`.
* [breaking] `Error::PostDeserialization` renamed to `Error::Finalization`.
* [breaking] `json-core` removed from default features.
* [breaking] Bumped MSRV to 1.70.0

### Added

* Python MQTT lib: Support for clearing a retained setting
* Python MQTT CLI: get() support
* `TreeKey::iter_indices()` and `iter_indices_unchecked()`
* Derive macros: Support for fallible getter/setter/validation callbacks
* Support for bit-packed keys `Packed` and `iter_packed()`/`iter_packed_unchecked()`
* A `postcard` feature and `Postcard` trait and blanket implementation
* `TreeKey::len()`
* The `typ` derive macro attribute

## [0.9.0](https://github.com/quartiq/miniconf/compare/v0.8.0...v0.9.0) - 2023-11-01

### Changed

* The `Miniconf` trait has been split into `TreeKey` for the keys/path/indices and traversal,
  the `TreeSerialize` for serialization, and `TreeDeserialize` for deserialization.
  The derive macros have been split accordingly. A shorthand `#[derive(Tree)]` macro has been
  added to derive all three traits. The struct field attribute controlling
  recursion depth has been renamed to `#[tree(depth(Y))]`.
* [mqtt] The `List` command of the `MqttClient` now has a maximum correlation data length of 32 bytes
* [mqtt] The `MqttClient` API has changed to support new Minimq versions
* [mqtt] The `Get` command now only generates a single message in response to the provided
  ResponseTopic instead of a response type (with success) and a message on the original topic.
* [mqtt] Handler function singatures now require `Display` instead of `AsRef<str>` types

### Added

* Deserializing with borrowed data is now supported.
* [derive] Added `#[tree(skip)]` macro attribute to allow skipping entries.

## [0.8.0](https://github.com/quartiq/miniconf/compare/v0.7.1...v0.8.0) - 2023-08-03

### Added

* Traversal by names or indices has been added through `Miniconf::traverse_by_key()`.
* The `Miniconf` derive macro supports (unnamed) tuple structs.

### Removed

* [breaking] The `Array` and `Option` newtypes have been removed. Instead in structs
  the desired `Miniconf<N>` recursion depth for a field is indicated by an attribute
  `#[miniconf(defer(N))]` where `N` is a `usize` literal. The depth is communicated
  via the trait. For `[T;N]` and `Option` the depth up to `8` has been implemented.
  For `structs` it is arbitrary.

### Changed

* [breaking] The `Miniconf` trait is now generic over the `Deserializer`/`Serializer`. It
  doesn't enforce `serde-json-core` or `u8` buffers or `/` as the path hierarchy
  separator anymore.
* [breaking] `Miniconf::iter_paths()` takes the path hierarchy separator and passes
  it on to `Miniconf::path()` and `Metadata::separator()`.
* [breaking] The `Miniconf` trait has been stripped of the provided functions that depended
  on the `serde`-backend and path hierarchy separator. Those have been
  moved into the `JsonCoreSlash` trait that has been implemented for all `Miniconf`
  to provide the previously existing functionality.
* [breaking] `set()` and `get()` have been renamed to `set_json()` and `get_json()`
  respectively to avoid overlap.
* [breaking] Paths now start with the path separator (unless they are empty).
  This affects the `Miniconf` derive macro and the `Miniconf` implementation pairs
  for `Option`/`Array`.
  Downstram crates should ensure non-empty paths start with the separator and
  expect `next_path` paths to start with the separator or be empty.
* The main serialization/deserialization methods are now `Miniconf::{set,get}_by_key()`
  They are generic over the key iterator `Iterator<Item: miniconf::Key>`.
* The only required change for most direct downstream users the `Miniconf` trait
  to adapt to the above is to make sure the `JsonCoreSlash` trait is in scope
  (`use miniconf::JsonCoreSlash`) and to rename `{set,get}() -> {set,get}_json()`.
  The `MqttClient` has seen no externally visible changes.
* [breaking] `iter_paths()` and `iter_paths_unchecked()` now don't need the state
  size anymore as it's computed exactly at compile time.
* [breaking] `iter_paths`/`PathIter` is now generic over the type
  to write the path into. Downstream crates should replace `iter_paths::<L, TS>()` with
  e.g. `iter_paths::<heapless::String<TS>>()`.
* [breaking] Re-exports of `heapless` and `serde-json-core` have been removed as they
  are not needed to work with the public API and would be a semver hazard.
* [breaking] Metadata is now computed by default without taking into account
  path separators. These can be included using `Metadata::separator()`.

## [0.7.1](https://github.com/quartiq/miniconf/compare/v0.7.0...v0.7.1)

### Fixed

* [MQTT] Now only subscribes to the `settings/#` and `list` topics to avoid unnecessary
  MQTT traffic and logging messages.
* [MQTT] Logging messages about omitted responses in case of missing `ResponseTopic` have been removed.

## [0.7.0](https://github.com/quartiq/miniconf/compare/v0.6.3...v0.7.0)

### Added

* [MQTT client] Getting values is now supported by publishing an empty message to the topic.
* [MQTT client] New `list` command is exposed under the Miniconf prefix to allow host software to
  discover current device settings tree structure.
* Python client updated to deprecate `command` in favor of `set`
* Python client now exposes `get()`, `set()`, and `list_paths()` APIs
* `AsRef`, `AsMut`, `IntoIterator` for `Array` and `Option`.
* Updated to minimq 0.7

### Changed

* Responses now encode status values as strings in a `UserProperty` with key "code"

### Fixed

* `miniconf::Option`'s `get_path()` and `set_path()` return `Err(Error::PathAbsent)`
  if `None`

## [0.6.3](https://github.com/quartiq/miniconf/compare/v0.6.2...v0.6.3) - 2022-12-09

* `Array` and `Option` are `repr(transparent)`
* Fixed documentation for `Array` and `Option`

## [0.6.2](https://github.com/quartiq/miniconf/compare/v0.6.1...v0.6.2) - 2022-11-09

* Renaming and reorganization of the the derive macro

## [0.6.1](https://github.com/quartiq/miniconf/compare/v0.6.0...v0.6.1) - 2022-11-04

* Documentation updates.

## [0.6.0](https://github.com/quartiq/miniconf/compare/v0.5.0...v0.6.0) - 2022-11-04

### Changed

* python module: don't emite whitespace in JSON to match serde-json-core (#92)
* `heapless::String` now implements `Miniconf` directly.
* Python client API is no longer retain by default. CLI is unchanged
* [breaking] Support for `#[derive(MiniconfAtomic)]` was removed.
* Fields in `#[derive(Miniconf)]` are now atomic by default. To recurse, users must
  annotate fields with `#[miniconf(defer)]`
* New `miniconf::Option` type has been added. Existing `Option` implementation has been changed to
  allow run-time nullability of values for more flexibility.
* New `miniconf::Array` type has been added, replacing the previous [T; N] implementation
* `Miniconf` implementation on most primitive types has been removed as it is no longer required.
* [breaking] The API has changed to be agnostic to usage (e.g. now referring to namespace paths and values
  instead of topics and settings). Functions in the `Miniconf` trait have been renamed.
* [breaking] Errors and the Metadata struct have beem marked `#[non_exhaustive]`
* [breaking] `metadata()`, `unchecked_iter_paths()`, `iter_paths()`, `next_path()` are
  all associated functions now.
* [breaking] Path iteration has been changed to move ownership of the iteration state into the iterator.
  And the path depth is now a const generic.
* [breaking] Path iteration will always return all paths regardless of potential runtime `miniconf::Option`
  or deferred `Option` being `None`.
* [breaking] `unchecked_iter_paths()` takes an optional iterator size to be used in `Iterator::size_hint()`.
* MQTT client now publishes responses with a quality of service of at-least-once to ensure
  transmission.
* MQTT client no longer uses correlation data to ignore local transmissions.

### Fixed

* Python device discovery now only discovers unique device identifiers. See [#97](https://github.com/quartiq/miniconf/issues/97)
* Python requests API updated to use a static response topic
* Python requests now have a timeout
* Generic trait bound requirements have been made more specific.

## [0.5.0] - 2022-05-12

### Changed

* **breaking** The Miniconf trait for iteration was renamed from `unchecked_iter()` and `iter()` to
  `unchecked_iter_settings()` and `iter_settings()` respectively to avoid issues with slice iteration
  name conflicts. See [#87](https://github.com/quartiq/miniconf/issues/87)

## [0.4.0] - 2022-05-11

### Added

* Added support for custom handling of settings updates.
* `Option` support added to enable run-time settings tree presence.

### Changed

* [breaking] MqttClient constructor now accepts initial settings values.
* Settings republish will no longer register as incoming configuration requests. See
  [#71](https://github.com/quartiq/miniconf/issues/71)
* [breaking] `into_iter()` and `unchecked_into_iter()` renamed to `iter()` and `unchecked_iter()`
  respectively to conform with standard conventions.

### Removed

* The client no longer resets the republish timeout when receiving messages.

## [0.3.0] - 2021-12-13

### Added

* Added key iteration
* Added support for retrieving serialized values via keys.
* Added APIs to the Miniconf trait for asynchronous iteration.
* Added publication of connectivity (alive) state to `<prefix>/alive` using MQTT will messages.
* Added automatic discovery of prefixes to CLI.
* Support for republishing settings after a predefined delay.

### Changed

* `miniconf::update()` replaced with `Miniconf::set()`, which is part of the trait and now
  directly available on structures.

## [0.2.0] - 2021-10-28

### Added

* Added support for generic maximum MQTT message size
* `derive_miniconf` added support for generic types

### Changed

* Updated minimq dependency to support ping TCP reconnection support

## [0.1.0] - 2021-08-11

Library initially released on crates.io

[0.5.0]: https://github.com/quartiq/miniconf/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/quartiq/miniconf/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/quartiq/miniconf/releases/tag/v0.3.0
[0.2.0]: https://github.com/quartiq/miniconf/releases/tag/v0.2.0
[0.1.0]: https://github.com/quartiq/miniconf/releases/tag/v0.1.0
