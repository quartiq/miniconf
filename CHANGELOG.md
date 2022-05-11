# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/quartiq/miniconf/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/quartiq/miniconf/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/quartiq/miniconf/releases/tag/v0.3.0
[0.2.0]: https://github.com/quartiq/miniconf/releases/tag/v0.2.0
[0.1.0]: https://github.com/quartiq/miniconf/releases/tag/v0.1.0
