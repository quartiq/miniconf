<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased

### Fixed

* Reduce CLI startup time and MQTT message latency.
* Schema-aware settings watches now raise an error when the device goes offline or its
  publication manifest changes, even when no settings arrive.

## [v0.21.0](https://github.com/quartiq/miniconf/compare/miniconf-v0.20.1...miniconf-mqtt-v0.21.0) - 2026-09-22

### Changed

* The CLI and client library now use the Miniconf MQTT retained schema/settings protocol and
  an async, context-managed API.

### Added

* Schema rendering, retained settings snapshots and watches, and stale retained-state pruning.

### Removed

* The synchronous client API (`miniconf.sync`).
