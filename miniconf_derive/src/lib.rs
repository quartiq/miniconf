#![warn(missing_docs)] // avoid hits for tests/examples but see alwo workspace lints

//! Derive macros for `miniconf` trees.
//!
//! Most users import these macros through `miniconf` and write `#[derive(Tree)]`.
//! `Tree` is shorthand for `TreeSchema`, `TreeSerialize`, `TreeDeserialize`, and
//! `TreeAny` on the same item.
//!
//! # Tree Shape
//!
//! Fields and variants are internal nodes when their types implement the relevant
//! `Tree*` traits. Serde leaves are accessed directly. Use
//! `#[tree(with = miniconf::leaf)]` to force a `Tree`-capable type to stay one
//! leaf value.
//!
//! ```ignore
//! use miniconf::{leaf, Tree};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Calibration {
//!     offset: i32,
//!     scale: u16,
//! }
//!
//! #[derive(Tree)]
//! struct Settings {
//!     #[tree(rename = "cal", with = leaf)]
//!     calibration: Calibration,
//! }
//! ```
//!
//! # Attributes
//!
//! `#[tree(...)]` is accepted on containers, fields, and variants:
//!
//! - `rename = "name"` exposes a different path segment.
//! - `skip` removes the field or variant from the tree.
//! - `flatten` splices one child tree into the parent when lookup is unambiguous.
//! - `with = module` delegates schema, serialization, deserialization, and `Any`
//!   access to functions in `module`.
//! - `meta(key = "value")` attaches reflection metadata.
//! - `meta(key)` inherits supported metadata from Rust syntax: `doc`,
//!   `typename`, or `nullable`.
//!
//! Container `meta(doc)` stores Rust doc comments as node metadata.
//! `meta(typename)` stores the Rust type name. Field and variant metadata is
//! edge metadata unless the item is flattened into the parent.
//!
//! ```ignore
//! use miniconf::Tree;
//!
//! /// Node documentation copied by `meta(doc)`.
//! #[derive(Tree)]
//! #[tree(meta(doc, typename))]
//! struct Settings {
//!     #[tree(rename = "en")]
//!     enabled: bool,
//!     #[tree(skip)]
//!     cache_only: u32,
//! }
//! ```
//!
//! # Custom Access
//!
//! `with = module` is the escape hatch for validation, read-only leaves, relaxed
//! bounds, or nonstandard access. The module exports the operations the derive
//! calls, such as `schema::<T>()`, `serialize_by_key`, `deserialize_by_key`,
//! `probe_by_key`, `ref_any_by_key`, and `mut_any_by_key`.
//!
//! Custom deserialize bounds may refer to the generated deserialize lifetime as
//! `'__de`.
//!
//! Prefer this for real access policy. Keep ordinary Serde leaves on the default
//! path or use `with = miniconf::leaf`.
//!
//! # Limits
//!
//! - Internal tree enums support unit, newtype, and skipped variants only.
//! - Enums with named fields or multi-field tuple variants should stay leaves or
//!   use a manual/custom implementation.
//! - Flattening is supported only when generated lookup stays unambiguous.

use darling::FromDeriveInput;
use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod field;
mod tree;
use tree::Tree;

/// Derive the `TreeSchema` trait for a struct or enum.
#[proc_macro_derive(TreeSchema, attributes(tree))]
pub fn derive_tree_schema(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_schema(),
        Err(e) => e.write_errors(),
    }
    .into()
}

/// Derive the `TreeSerialize` trait for a struct or enum.
#[proc_macro_derive(TreeSerialize, attributes(tree))]
pub fn derive_tree_serialize(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_serialize(),
        Err(e) => e.write_errors(),
    }
    .into()
}

/// Derive the `TreeDeserialize` trait for a struct or enum.
#[proc_macro_derive(TreeDeserialize, attributes(tree))]
pub fn derive_tree_deserialize(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_deserialize(),
        Err(e) => e.write_errors(),
    }
    .into()
}

/// Derive the `TreeAny` trait for a struct or enum.
#[proc_macro_derive(TreeAny, attributes(tree))]
pub fn derive_tree_any(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_any(),
        Err(e) => e.write_errors(),
    }
    .into()
}

/// Derive the `TreeSchema`, `TreeSerialize`, `TreeDeserialize`, and `TreeAny` traits for a struct or enum.
///
/// This is a shorthand to derive multiple traits.
#[proc_macro_derive(Tree, attributes(tree))]
pub fn derive_tree(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => [
            t.tree_schema(),
            t.tree_serialize(),
            t.tree_deserialize(),
            t.tree_any(),
        ]
        .into_iter()
        .collect(),
        Err(e) => e.write_errors(),
    }
    .into()
}
