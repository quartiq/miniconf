# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/quartiq/miniconf/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/quartiq/miniconf/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/quartiq/miniconf/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/quartiq/miniconf/releases/tag/v0.3.0
[0.2.0]: https://github.com/quartiq/miniconf/releases/tag/v0.2.0
[0.1.0]: https://github.com/quartiq/miniconf/releases/tag/v0.1.0
