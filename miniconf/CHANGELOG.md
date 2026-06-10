<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED](https://github.com/quartiq/miniconf/compare/miniconf-v0.21.0...HEAD) - DATE

## [0.21.0](https://github.com/quartiq/miniconf/compare/miniconf-v0.20.1...miniconf-v0.21.0) - 2026-06-10

### Changed

* `Path` and `PathIter` now store the separator at runtime, reducing monomorphization bloat for
  dynamic path handling.
* Key handling now uses `IntoKeys` as the boundary normalization layer. Slash-separated `&str`
  remains the default shorthand, while explicit separators use `PathIter` or `ConstPathIter`.
* `Schema::get()` now performs exact lookup and returns `Lookup`; use `Schema::resolve_into()` for
  consumed-depth reporting on partial or failed lookup.
* `Schema::transcode()` now takes `impl IntoKeys` and default-constructs the target.
* `NodeIter` now yields leaves only. Depth-limited iteration skips leaves deeper than the limit,
  and iterator position is exposed through `indices()`, `schema()`, and `root_schema()`.
* `Schema` is now a public enum with explicit `Leaf` and `Internal` variants backed by shared
  `NodeSchema` and `InternalSchema`; schema nodes now have stable constructors and accessors.
* `Meta` is now an always-available newtype with direct serialization support, and schema metadata
  terminology is now consistently `node` and `edge`.
* `Schema::get_meta()` now returns both edge and node metadata for one path.
* JSON Schema output now marks Miniconf leaves explicitly and better matches the emitted JSON tree
  for omitted named absences, nullable leaves, and `oneOf` nodes.

### Added

* `ConstPath` and `ConstPathIter` take the separator as a const generic, allowing compile-time
  specialization of ASCII separators.
* `json_core::{get_by_keys, set_by_keys}` and `postcard::{get_by_keys, set_by_keys}` for live
  key cursors.
* `Keys` for borrowed slices `&[T]` where `T: Key`.
* `Sem` offers semantic structured information about nodes in a `Schema`.
* Structured `sem.maybe_absent` on `Option<T>` schema nodes and `sem.oneof` on derived enums,
  `Result<T, E>`, and `Bound<T>`.
* `miniconf/tests/benchmark` is now the embedded code-size and workload benchmark harness, with
  `baseline`, `manual`, and `miniconf` binaries plus schema-size measurement support.

### Removed

* `meta-str`.
