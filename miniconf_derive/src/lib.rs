use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, DeriveInput};

mod field;

/// Derive the `TreeKey` trait for a struct.
#[proc_macro_derive(TreeKey, attributes(tree))]
pub fn derive_tree_key(input: TokenStream) -> TokenStream {
    let mut tree = match field::Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t,
        Err(e) => {
            return e.write_errors().into();
        }
    };

    tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeKey<#depth>))
        } else {
            None
        }
    });

    let fields = tree.fields();
    let fields_len = fields.len();

    let (names, name_to_index, index_to_name, index_len) =
        if fields.iter().all(|f| f.ident.is_none()) {
            (
                None,
                quote!(str::parse(value).ok()),
                quote!(if index >= #fields_len {
                    Err(::miniconf::Traversal::NotFound(1))?
                } else {
                    None
                }),
                quote!(index.checked_ilog10().unwrap_or_default() as usize + 1),
            )
        } else {
            let names = fields.iter().map(|field| {
                // ident is Some
                let name = field.name().unwrap();
                quote! { stringify!(#name) }
            });
            (
                Some(quote!(
                    const __MINICONF_NAMES: &'static [&'static str] = &[#(#names ,)*];
                )),
                quote!(Self::__MINICONF_NAMES.iter().position(|&n| n == value)),
                quote!(Some(
                    *Self::__MINICONF_NAMES
                        .get(index)
                        .ok_or(::miniconf::Traversal::NotFound(1))?
                )),
                quote!(Self::__MINICONF_NAMES[index].len()),
            )
        };

    let traverse_by_key_arms = fields
        .iter()
        .enumerate()
        .filter_map(|(i, field)| field.traverse_by_key(i));
    let metadata_arms = fields
        .iter()
        .enumerate()
        .filter_map(|(i, field)| field.metadata(i));
    let defers = fields.iter().map(|field| field.depth > 0);
    let depth = tree.depth();
    let ident = &tree.ident;

    let (impl_generics, ty_generics, where_clause) = tree.generics.split_for_impl();

    quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            // TODO: can these be hidden and disambiguated w.r.t. collision?
            fn __miniconf_lookup<K: ::miniconf::Keys>(keys: &mut K) -> Result<usize, ::miniconf::Traversal> {
                const DEFERS: [bool; #fields_len] = [#(#defers ,)*];
                let index = ::miniconf::Keys::next::<Self>(keys)?;
                let defer = DEFERS.get(index)
                    .ok_or(::miniconf::Traversal::NotFound(1))?;
                if !defer && !keys.finalize() {
                    Err(::miniconf::Traversal::TooLong(1))
                } else {
                    Ok(index)
                }
            }

            #names
        }

        impl #impl_generics ::miniconf::KeyLookup for #ident #ty_generics #where_clause {
            const LEN: usize = #fields_len;

            #[inline]
            fn name_to_index(value: &str) -> Option<usize> {
                #name_to_index
            }
        }

        impl #impl_generics ::miniconf::TreeKey<#depth> for #ident #ty_generics #where_clause {
            fn metadata() -> ::miniconf::Metadata {
                let mut meta = ::miniconf::Metadata::default();
                for index in 0..#fields_len {
                    let item_meta: ::miniconf::Metadata = match index {
                        #(#metadata_arms ,)*
                        _ => {
                            let mut m = ::miniconf::Metadata::default();
                            m.count = 1;
                            m
                        }
                    };
                    meta.max_length = meta.max_length.max(
                        #index_len +
                        item_meta.max_length
                    );
                    meta.max_depth = meta.max_depth.max(
                        item_meta.max_depth
                    );
                    meta.count += item_meta.count;
                }
                meta.max_depth += 1;
                meta
            }

            fn traverse_by_key<K, F, E>(
                mut keys: K,
                mut func: F,
            ) -> Result<usize, ::miniconf::Error<E>>
            where
                K: ::miniconf::Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                let index = ::miniconf::Keys::next::<Self>(&mut keys)?;
                let name = #index_to_name;
                func(index, name, #fields_len).map_err(|err| ::miniconf::Error::Inner(1, err))?;
                ::miniconf::Error::increment_result(match index {
                    #(#traverse_by_key_arms ,)*
                    _ => {
                        if !keys.finalize() {
                            Err(::miniconf::Traversal::TooLong(0).into())
                        } else {
                            Ok(0)
                        }
                    }
                })
            }
        }
    }
    .into()
}

/// Derive the `TreeSerialize` trait for a struct.
#[proc_macro_derive(TreeSerialize, attributes(tree))]
pub fn derive_tree_serialize(input: TokenStream) -> TokenStream {
    let mut tree = match field::Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t,
        Err(e) => {
            return e.write_errors().into();
        }
    };

    tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeSerialize<#depth>))
        } else {
            Some(parse_quote!(::miniconf::Serialize))
        }
    });

    let serialize_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.serialize_by_key(i));
    let depth = tree.depth();
    let ident = &tree.ident;

    let (impl_generics, ty_generics, where_clause) = tree.generics.split_for_impl();

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

/// Derive the `TreeDeserialize` trait for a struct.
#[proc_macro_derive(TreeDeserialize, attributes(tree))]
pub fn derive_tree_deserialize(input: TokenStream) -> TokenStream {
    let mut tree = match field::Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t,
        Err(e) => {
            return e.write_errors().into();
        }
    };

    tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeDeserialize<'de, #depth>))
        } else {
            Some(parse_quote!(::miniconf::Deserialize<'de>))
        }
    });

    let depth = tree.depth();
    let ident = &tree.ident;

    let orig_generics = tree.generics.clone();
    let (_, ty_generics, where_clause) = orig_generics.split_for_impl();
    let lts: Vec<_> = tree.generics.lifetimes().cloned().collect();
    tree.generics.params.push(parse_quote!('de));
    if let Some(syn::GenericParam::Lifetime(de)) = tree.generics.params.last_mut() {
        assert_eq!(de.lifetime.ident, "de");
        for l in lts {
            assert!(l.lifetime.ident != "de");
            de.bounds.push(l.lifetime);
        }
    }
    let (impl_generics, _, _) = tree.generics.split_for_impl();

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

/// Derive the `TreeAny` trait for a struct.
#[proc_macro_derive(TreeAny, attributes(tree))]
pub fn derive_tree_any(input: TokenStream) -> TokenStream {
    let mut tree = match field::Tree::parse(&parse_macro_input!(input as DeriveInput)) {
        Ok(t) => t,
        Err(e) => {
            return e.write_errors().into();
        }
    };

    tree.bound_generics(&mut |depth| {
        if depth > 0 {
            Some(parse_quote!(::miniconf::TreeAny<#depth>))
        } else {
            Some(parse_quote!(::core::any::Any))
        }
    });

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
    let depth = tree.depth();
    let ident = &tree.ident;

    let (impl_generics, ty_generics, where_clause) = tree.generics.split_for_impl();

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

/// Shorthand to derive the `TreeKey`, `TreeAny`, `TreeSerialize`, and `TreeDeserialize` traits for a struct.
#[proc_macro_derive(Tree, attributes(tree))]
pub fn derive_tree(input: TokenStream) -> TokenStream {
    let mut t = derive_tree_key(input.clone());
    t.extend(derive_tree_any(input.clone()));
    t.extend(derive_tree_serialize(input.clone()));
    t.extend(derive_tree_deserialize(input));
    t
}
