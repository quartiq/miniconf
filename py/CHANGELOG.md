<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED](https://github.com/quartiq/miniconf/compare/miniconf-v0.20.1...HEAD) - DATE

### Fixed

* Clean up failed or cancelled requests and subscriptions; malformed replies fail only their request.
* Snapshots and pruning report timeout failure when retained traffic has not reached quiescence.

### Changed

* The CLI and client library now use the Miniconf MQTT retained schema/settings protocol and async API.
* Human-readable trees label empty-name path segments as `""`, distinguishing the `/` child from
  the empty root path.

### Added

* Schema rendering, retained settings snapshots and watches, and stale retained-state pruning.

### Removed

* The synchronous client API (`miniconf.sync`).
