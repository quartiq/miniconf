<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED](https://github.com/quartiq/miniconf/compare/miniconf_derive-v0.21.0...HEAD) - DATE

## [0.21.0](https://github.com/quartiq/miniconf/compare/miniconf_derive-v0.20.0...miniconf_derive-v0.21.0) - 2026-06-10

### Changed

* Custom `#[tree(with = ...)]` modules now expose typed schema via `schema::<T>()` instead of a
  monomorphic `SCHEMA` constant.

### Added

* `#[tree(meta(...))]` for derive metadata syntax.
* `nullable` as an edge metadata hint, e.g. `#[tree(with = leaf, meta(nullable))]`, propagated
  into JSON Schema as `null`.
