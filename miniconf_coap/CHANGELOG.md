<!-- markdownlint-disable MD024 -->
# Changelog

All notable changes to this package will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [UNRELEASED](https://github.com/quartiq/miniconf/compare/miniconf_coap-v0.2.0...HEAD) - DATE

### Changed

* Breaking: `SchemaRoute::new` takes a fixed page size, independent of response buffer size.
* Breaking: error responses use optional UTF-8 diagnostics instead of JSON/CBOR;
  `Problem` consolidates option errors into `BadOption` and renames `UriPathTooLong` to `PathCapacity`.
* Breaking: `Outcome::Written` and `LeafKey` replace `Outcome::Changed` and `ChangedKey`;
  handler request data exposes the written key.
* Breaking: `Response` includes `max_age`; live values use `Max-Age: 0`.

### Fixed

* Correct handling of repeated and malformed Accept/Content-Format options.
* Reject malformed CBOR payloads before updating ordinary leaves.

## [0.2.0](https://github.com/quartiq/miniconf/compare/miniconf_coap-v0.1.0...miniconf_coap-v0.2.0) - 2026-09-25

## [0.1.0](https://github.com/quartiq/miniconf/compare/miniconf-v0.20.1...miniconf_coap-v0.1.0) - 2026-06-10

### Added

* `miniconf_coap`, a new CoAP transport crate with low-level request handlers, JSON and CBOR value
  representations, compact schema serving, `coap-handler` integration, and a packet-level server
  example.
