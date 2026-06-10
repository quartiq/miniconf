<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED](https://github.com/quartiq/miniconf/compare/miniconf-v0.20.1...HEAD) - DATE

### Changed

* The Python package was restructured from `py/miniconf-mqtt` to `py/` and now targets the MM2
  retained schema/settings protocol with an async-first CLI and client library.

### Added

* MM2 schema parsing/rendering helpers and one-shot retained-state operations such as `read()`,
  `dump()`, `prune()`, and `force_prune()`.

### Removed

* The Python synchronous client surface (`miniconf.sync`) and the old package layout under
  `py/miniconf-mqtt`.
