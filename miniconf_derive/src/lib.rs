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
                    Err(::miniconf::Error::NotFound(1))?
                } else {
                    None
                }),
                quote!(::miniconf::digits::<10>(index)),
            )
        } else {
            let names = fields.iter().map(|field| {
                let name = field.name().unwrap();
                quote! { stringify!(#name) }
            });
            (
                Some(quote!(const __MINICONF_NAMES: [&'static str; #fields_len] = [#(#names ,)*];)),
                quote!(Self::__MINICONF_NAMES.iter().position(|&n| n == value)),
                quote!(Some(
                    *Self::__MINICONF_NAMES
                        .get(index)
                        .ok_or(::miniconf::Error::NotFound(1))?
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
            #names
            const __MINICONF_DEFERS: [bool; #fields_len] = [#(#defers ,)*];
        }

        impl #impl_generics ::miniconf::TreeKey<#depth> for #ident #ty_generics #where_clause {
            #[inline]
            fn len() -> usize {
                #fields_len
            }

            #[inline]
            fn name_to_index(value: &str) -> Option<usize> {
                #name_to_index
            }

            fn traverse_by_key<K, F, E>(
                mut keys: K,
                mut func: F,
            ) -> Result<usize, ::miniconf::Error<E>>
            where
                K: ::miniconf::Keys,
                F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
            {
                let index = ::miniconf::Keys::lookup::<#depth, Self, _>(&mut keys)?;
                let name = #index_to_name;
                func(index, name, Self::len())?;
                ::miniconf::increment_result(match index {
                    #(#traverse_by_key_arms ,)*
                    _ => Ok(0),
                })
            }

            fn metadata() -> ::miniconf::Metadata {
                let mut meta = ::miniconf::Metadata::default();
                for index in 0..Self::len() {
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
                let index = ::miniconf::Keys::lookup::<#depth, Self, _>(&mut keys)?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                if !defer && !::miniconf::Keys::is_empty(&mut keys) {
                    return Err(::miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                ::miniconf::increment_result(match index {
                    #(#serialize_by_key_arms ,)*
                    _ => unreachable!()
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
                let index = ::miniconf::Keys::lookup::<#depth, Self, _>(&mut keys)?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                if !defer && !::miniconf::Keys::is_empty(&mut keys) {
                    return Err(::miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                ::miniconf::increment_result(match index {
                    #(#deserialize_by_key_arms ,)*
                    _ => unreachable!()
                })
            }
        }
    }.into()
}

/// Derive the `TreeDeserialize` trait for a struct.
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

    let get_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.get_by_key(i));
    let get_mut_by_key_arms = tree
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| field.get_mut_by_key(i));
    let depth = tree.depth();
    let ident = &tree.ident;

    let (impl_generics, ty_generics, where_clause) = tree.generics.split_for_impl();

    quote! {
        impl #impl_generics ::miniconf::TreeAny<#depth> for #ident #ty_generics #where_clause {
            fn get_by_key<K>(&self, mut keys: K) -> Result<&dyn ::core::any::Any, ::miniconf::Error<()>>
            where
                K: ::miniconf::Keys,
            {
                let index = ::miniconf::Keys::lookup::<#depth, Self, _>(&mut keys)?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                if !defer && !::miniconf::Keys::is_empty(&mut keys) {
                    return Err(::miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                {
                    let ret: Result<_, _> = match index {
                        #(#get_by_key_arms ,)*
                        _ => unreachable!()
                    };
                    ret.map_err(::miniconf::Error::increment)
                }
            }

            fn get_mut_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn ::core::any::Any, ::miniconf::Error<()>>
            where
                K: ::miniconf::Keys,
            {
                let index = ::miniconf::Keys::lookup::<#depth, Self, _>(&mut keys)?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                if !defer && !::miniconf::Keys::is_empty(&mut keys) {
                    return Err(::miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                {
                    let ret: Result<_, _> = match index {
                        #(#get_mut_by_key_arms ,)*
                        _ => unreachable!()
                    };
                    ret.map_err(::miniconf::Error::increment)
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
