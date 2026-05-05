#![warn(missing_docs)] // avoid hits for tests/examples but see alwo workspace lints

//! Derive macros for `miniconf` trees.
//!
//! Most users derive `Tree` from the main `miniconf` crate. That shorthand implements
//! `TreeSchema`, `TreeSerialize`, `TreeDeserialize`, and `TreeAny` for one item.
//!
//! # Tree shape
//!
//! Struct fields, tuple fields, enum variants, arrays, tuples, and `Option<T>` can become
//! internal nodes when their types also implement the relevant `Tree*` traits. Leaf fields use
//! Serde and `Any` directly.
//!
//! Use `#[tree(with = miniconf::leaf)]` to force a `Tree`-capable type to stay a leaf.
//!
//! # Attributes
//!
//! The derive recognizes `#[tree(...)]` on containers, fields, and variants:
//!
//! - `rename = name`: expose a different path segment.
//! - `skip`: remove a field or variant from the tree.
//! - `flatten`: splice one child tree into the parent where lookup is unambiguous.
//! - `with = module`: use a custom implementation module for this field.
//! - `meta(key = "value")`: attach reflection metadata.
//! - `meta(key)`: inherit supported metadata (`doc`, `typename`, or `nullable`) from Rust syntax.
//!
//! Container-level `meta(doc)` copies Rust doc comments into node metadata. `meta(typename)`
//! records the Rust type name. Field and variant metadata is edge metadata unless the item is
//! flattened into the parent.
//!
//! Limitations:
//! - internal tree enums are limited to unit and newtype variants
//! - flattening is only supported where lookup stays unambiguous

use darling::FromDeriveInput;
use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod field;
mod tree;
use tree::Tree;

/// Derive the `TreeSchema` trait for a struct or enum.
///
/// This also derives `KeyLookup` if necessary.
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
