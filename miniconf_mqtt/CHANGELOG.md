<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED](https://github.com/quartiq/miniconf/compare/miniconf_mqtt-v0.21.0...HEAD) - DATE

## [0.21.0](https://github.com/quartiq/miniconf/compare/miniconf_mqtt-v0.20.0...miniconf_mqtt-v0.21.0) - 2026-06-10

### Changed

* `miniconf_mqtt` is now async-first on caller-owned `minimq::Session`. The old `update()` loop
  was replaced by `Miniconf::{startup,serve}` plus the stepwise `LoadRetained`, `Startup`,
  `Service`, and `Publisher` APIs.
* MM2 now publishes retained manifest, paged schema, and authoritative retained settings.
  Compatibility with direct `settings/#` writes is available when applications explicitly
  subscribe to and route those publications through `Service`.
* MM2 manifests now publish `epoch` and `schema_rev`. Long-lived clients use `schema_rev` to
  invalidate cached schema.

### Added

* `Publisher::{root,by_key}` for explicit app-driven retained settings publication.

### Removed

* The legacy `MqttClient` API.
