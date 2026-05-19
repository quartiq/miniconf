# `miniconf` Derive Macros

This package contains the derive macros re-exported by
[`miniconf`](https://crates.io/crates/miniconf).

Most users should depend on `miniconf` and write `#[derive(Tree)]` there. `Tree`
derives `TreeSchema`, `TreeSerialize`, `TreeDeserialize`, and `TreeAny` together.

Attribute syntax and derive behavior are documented in this crate's rustdoc.

The important attributes are `rename`, `skip`, `flatten`, `with = module`, and
`meta(...)`. Use `with = miniconf::leaf` to keep a `Tree`-capable type as one
Serde leaf.

Internal tree enums support unit, newtype, and skipped variants. Enums with
named fields or multi-field tuple variants should stay leaves or use a
manual/custom implementation.
