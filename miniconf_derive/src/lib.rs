use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, DeriveInput};

mod field;
mod tree;
use tree::Tree;

fn do_derive_tree_serialize(tree: &Tree) -> TokenStream {
    let generics = tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeSerialize<#depth>))
        } else {
            Some(parse_quote!(::miniconf::Serialize))
        }
    });

    let depth = tree.depth();
    let ident = &tree.ident;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let serialize_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.serialize_by_key(i));

    quote! {
        impl #impl_generics ::miniconf::TreeSerialize<#depth> for #ident #ty_generics #where_clause {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, ::miniconf::Error<S::Error>>
            where
                K: ::miniconf::Keys,
                S: ::miniconf::Serializer,
            {
                let index = Self::__miniconf_lookup(&mut keys)?;
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                ::miniconf::Error::increment_result(match index {
                    #(#serialize_by_key_arms ,)*
                    _ => unreachable!(),
                })
            }
        }
    }.into()
}

fn do_derive_tree_deserialize(tree: &Tree) -> TokenStream {
    let mut generics = tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeDeserialize<'de, #depth>))
        } else {
            Some(parse_quote!(::miniconf::Deserialize<'de>))
        }
    });

    let depth = tree.depth();
    let ident = &tree.ident;

    let orig_generics = generics.clone();
    let (_, ty_generics, where_clause) = orig_generics.split_for_impl();
    let lts: Vec<_> = generics.lifetimes().cloned().collect();
    generics.params.push(parse_quote!('de));
    if let Some(syn::GenericParam::Lifetime(de)) = generics.params.last_mut() {
        assert_eq!(de.lifetime.ident, "de");
        for l in lts {
            assert!(l.lifetime.ident != "de");
            de.bounds.push(l.lifetime);
        }
    }
    let (impl_generics, _, _) = generics.split_for_impl();

    let deserialize_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.deserialize_by_key(i));

    quote! {
        impl #impl_generics ::miniconf::TreeDeserialize<'de, #depth> for #ident #ty_generics #where_clause {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, ::miniconf::Error<D::Error>>
            where
                K: ::miniconf::Keys,
                D: ::miniconf::Deserializer<'de>,
            {
                let index = Self::__miniconf_lookup(&mut keys)?;
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                ::miniconf::Error::increment_result(match index {
                    #(#deserialize_by_key_arms ,)*
                    _ => unreachable!(),
                })
            }
        }
    }.into()
}

fn do_derive_tree_any(tree: &Tree) -> TokenStream {
    let generics = tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeAny<#depth>))
        } else {
            Some(parse_quote!(::core::any::Any))
        }
    });

    let depth = tree.depth();
    let ident = &tree.ident;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let ref_any_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.ref_any_by_key(i));
    let mut_any_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.mut_any_by_key(i));

    quote! {
        impl #impl_generics ::miniconf::TreeAny<#depth> for #ident #ty_generics #where_clause {
            fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn ::core::any::Any, ::miniconf::Traversal>
            where
                K: ::miniconf::Keys,
            {
                let index = Self::__miniconf_lookup(&mut keys)?;
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                {
                    let ret: Result<_, _> = match index {
                        #(#ref_any_by_key_arms ,)*
                        _ => unreachable!()
                    };
                    ret.map_err(::miniconf::Traversal::increment)
                }
            }

            fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn ::core::any::Any, ::miniconf::Traversal>
            where
                K: ::miniconf::Keys,
            {
                let index = Self::__miniconf_lookup(&mut keys)?;
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                {
                    let ret: Result<_, _> = match index {
                        #(#mut_any_by_key_arms ,)*
                        _ => unreachable!()
                    };
                    ret.map_err(::miniconf::Traversal::increment)
                }
            }
        }
    }.into()
}

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
        Ok(t) => do_derive_tree_serialize(&t),
        Err(e) => e.write_errors().into(),
    }
}

/// Derive the `TreeDeserialize` trait for a struct.
#[proc_macro_derive(TreeDeserialize, attributes(tree))]
pub fn derive_tree_deserialize(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => do_derive_tree_deserialize(&t),
        Err(e) => e.write_errors().into(),
    }
}

/// Derive the `TreeAny` trait for a struct.
#[proc_macro_derive(TreeAny, attributes(tree))]
pub fn derive_tree_any(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => do_derive_tree_any(&t),
        Err(e) => e.write_errors().into(),
    }
}

/// Shorthand to derive the `TreeKey`, `TreeAny`, `TreeSerialize`, and `TreeDeserialize` traits for a struct.
#[proc_macro_derive(Tree, attributes(tree))]
pub fn derive_tree(input: TokenStream) -> TokenStream {
    match Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => {
            let mut ts: TokenStream = t.tree_key().into();
            ts.extend(do_derive_tree_any(&t));
            ts.extend(do_derive_tree_serialize(&t));
            ts.extend(do_derive_tree_deserialize(&t));
            ts
        }
        Err(e) => e.write_errors().into(),
    }
}
