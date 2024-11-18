use darling::FromDeriveInput;
use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod field;
mod tree;
use tree::Tree;

/// Derive the `TreeKey` trait for a struct or enum.
///
/// This also derives `KeyLookup` if necessary.
#[proc_macro_derive(TreeKey, attributes(tree))]
pub fn derive_tree_key(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_key(),
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

/// Derive the `TreeKey`, `TreeSerialize`, `TreeDeserialize`, and `TreeAny` traits for a struct or enum.
///
/// This is a shorthand to derive multiple traits.
#[proc_macro_derive(Tree, attributes(tree))]
pub fn derive_tree(input: TokenStream) -> TokenStream {
    match Tree::from_derive_input(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => [
            t.tree_key(),
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
