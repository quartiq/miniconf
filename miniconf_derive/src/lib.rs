use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod field;
mod tree;
use tree::Tree;

/// Derive the `TreeKey` trait for a struct.
#[proc_macro_derive(TreeKey, attributes(tree))]
pub fn derive_tree_key(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_key().into(),
        Err(e) => e.write_errors().into(),
    }
}

/// Derive the `TreeSerialize` trait for a struct.
#[proc_macro_derive(TreeSerialize, attributes(tree))]
pub fn derive_tree_serialize(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_serialize().into(),
        Err(e) => e.write_errors().into(),
    }
}

/// Derive the `TreeDeserialize` trait for a struct.
#[proc_macro_derive(TreeDeserialize, attributes(tree))]
pub fn derive_tree_deserialize(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_deserialize().into(),
        Err(e) => e.write_errors().into(),
    }
}

/// Derive the `TreeAny` trait for a struct.
#[proc_macro_derive(TreeAny, attributes(tree))]
pub fn derive_tree_any(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t.tree_any().into(),
        Err(e) => e.write_errors().into(),
    }
}

/// Shorthand to derive the `TreeKey`, `TreeAny`, `TreeSerialize`, and `TreeDeserialize` traits for a struct.
#[proc_macro_derive(Tree, attributes(tree))]
pub fn derive_tree(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => {
            let mut ts: TokenStream = t.tree_key().into();
            ts.extend(TokenStream::from(t.tree_any()));
            ts.extend(TokenStream::from(t.tree_serialize()));
            ts.extend(TokenStream::from(t.tree_deserialize()));
            ts
        }
        Err(e) => e.write_errors().into(),
    }
}
