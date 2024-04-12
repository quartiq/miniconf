use darling::FromDeriveInput;
use proc_macro::TokenStream;
// use proc_macro_error::abort;
use quote::quote;
use syn::{parse_macro_input, parse_quote, DeriveInput};

mod field;

fn name_or_index(i: usize, ident: &Option<syn::Ident>) -> proc_macro2::TokenStream {
    match ident {
        None => {
            let index = syn::Index::from(i);
            quote! { #index }
        }
        Some(name) => quote! { #name },
    }
}

/// Derive the `TreeKey` trait for a struct.
#[proc_macro_derive(TreeKey, attributes(tree))]
pub fn derive_tree_key(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);
    let tree = field::Tree::from_derive_input(&input).unwrap();
    let darling::ast::Data::Struct(mut fields) = tree.data else {
        unreachable!()
    };
    fields.fields.retain(|f| !f.skip.is_present());

    for f in fields.iter() {
        f.bound_generics(
            &mut |depth| {
                if depth > 0 {
                    Some(parse_quote!(::miniconf::TreeKey<#depth>))
                } else {
                    None
                }
            },
            &mut input.generics,
        )
    }

    let traverse_by_key_arms = fields.iter().enumerate().filter_map(|(i, field)| {
        // Quote context is a match of the field index with `traverse_by_key()` args available.
        let depth = field.depth();
        if depth > 0 {
            let field_type = &field.ty;
            Some(quote! {
                #i => <#field_type as ::miniconf::TreeKey<#depth>>::traverse_by_key(keys, func)
            })
        } else {
            None
        }
    });
    let metadata_arms = fields.iter().enumerate().filter_map(|(i, field)| {
        // Quote context is a match of the field index with `metadata()` args available.
        let depth = field.depth();
        if depth > 0 {
            let field_type = &field.ty;
            Some(quote! {
                #i => <#field_type as ::miniconf::TreeKey<#depth>>::metadata()
            })
        } else {
            None
        }
    });

    let names = fields.iter().enumerate().map(|(i, field)| {
        let name = name_or_index(i, &field.ident);
        quote! { stringify!(#name) }
    });
    let fields_len = fields.len();

    let defers = fields.iter().map(|field| field.depth() > 0);
    let depth = fields.iter().fold(0usize, |d, field| d.max(field.depth())) + 1;
    let ident = input.ident;

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            // TODO: can these be hidden and disambiguated w.r.t. collision?
            // TODO: for unnamed structs, simplify `["0", "1", "2"].position(|&n| n == value)`
            //       to `parse::<usize>(value)`
            const __MINICONF_NAMES: [&'static str; #fields_len] = [#(#names ,)*];
            const __MINICONF_DEFERS: [bool; #fields_len] = [#(#defers ,)*];
        }

        impl #impl_generics ::miniconf::TreeKey<#depth> for #ident #ty_generics #where_clause {
            fn name_to_index(value: &str) -> Option<usize> {
                Self::__MINICONF_NAMES.iter().position(|&n| n == value)
            }

            fn traverse_by_key<K, F, E>(
                mut keys: K,
                mut func: F,
            ) -> Result<usize, ::miniconf::Error<E>>
            where
                K: Iterator,
                K::Item: ::miniconf::Key,
                F: FnMut(usize, &str) -> Result<(), E>,
            {
                let key = keys.next()
                    .ok_or(::miniconf::Error::TooShort(0))?;
                let index = ::miniconf::Key::find::<#depth, Self>(&key)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                let name = Self::__MINICONF_NAMES.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                func(index, name)?;
                ::miniconf::Increment::increment(match index {
                    #(#traverse_by_key_arms ,)*
                    _ => Ok(0),
                })
            }

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
                        Self::__MINICONF_NAMES[index].len() +
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
    let mut input = parse_macro_input!(input as DeriveInput);
    let tree = field::Tree::from_derive_input(&input).unwrap();
    let darling::ast::Data::Struct(mut fields) = tree.data else {
        unreachable!()
    };
    fields.fields.retain(|f| !f.skip.is_present());

    for f in fields.iter() {
        f.bound_generics(
            &mut |depth| {
                if depth > 0 {
                    Some(parse_quote!(::miniconf::TreeSerialize<#depth>))
                } else {
                    Some(parse_quote!(::miniconf::Serialize))
                }
            },
            &mut input.generics,
        )
    }

    let serialize_by_key_arms = fields.iter().enumerate().map(|(i, field)| {
        // Quote context is a match of the field name with `serialize_by_key()` args available.
        let ident = name_or_index(i, &field.ident);
        let depth = field.depth();
        if depth > 0 {
            quote! {
                #i => ::miniconf::TreeSerialize::<#depth>::serialize_by_key(&self.#ident, keys, ser)
            }
        } else {
            quote! {
                #i => {
                    ::miniconf::Serialize::serialize(&self.#ident, ser).and(Ok(0)).map_err(|e| ::miniconf::Error::Inner(e))
               }
            }
        }
    });

    let depth = fields.iter().fold(0usize, |d, field| d.max(field.depth())) + 1;
    let ident = input.ident;

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::miniconf::TreeSerialize<#depth> for #ident #ty_generics #where_clause {
            fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, ::miniconf::Error<S::Error>>
            where
                K: Iterator,
                K::Item: ::miniconf::Key,
                S: ::miniconf::Serializer,
            {
                let key = keys.next()
                    .ok_or(::miniconf::Error::TooShort(0))?;
                let index = ::miniconf::Key::find::<#depth, Self>(&key)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                if !defer && keys.next().is_some() {
                    return Err(::miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                ::miniconf::Increment::increment(match index {
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
    let mut input = parse_macro_input!(input as DeriveInput);
    let tree = field::Tree::from_derive_input(&input).unwrap();
    let darling::ast::Data::Struct(mut fields) = tree.data else {
        unreachable!()
    };
    fields.fields.retain(|f| !f.skip.is_present());

    for f in fields.iter() {
        f.bound_generics(
            &mut |depth| {
                if depth > 0 {
                    Some(parse_quote!(::miniconf::TreeDeserialize<'de, #depth>))
                } else {
                    Some(parse_quote!(::miniconf::DeserializeOwned))
                }
            },
            &mut input.generics,
        )
    }

    let deserialize_by_key_arms = fields.iter().enumerate().map(|(i, field)| {
        // Quote context is a match of the field name with `deserialize_by_key()` args available.
        let ident = name_or_index(i, &field.ident);
        let depth = field.depth();
        if depth > 0 {
            let validate = match &field.validate {
                Some(validate) => quote!(
                    |i| #validate(&mut self.#ident, stringify!(#ident))
                        .and(Ok(i)).map_err(|msg| ::miniconf::Error::Invalid(0, msg))
                ),
                None => quote!(|i| Ok(i)),
            };
            quote! {
                #i => {
                    ::miniconf::TreeDeserialize::<'de, #depth>::deserialize_by_key(&mut self.#ident, keys, de)
                        .and_then(#validate)
                }
            }
        } else {
            let validate = match &field.validate {
                Some(validate) => quote!(
                    |v| #validate(&self, v, stringify!(#ident), &self.#ident)
                        .map_err(|msg| ::miniconf::Error::Invalid(0, msg))),
                None => quote!(|v| Ok(v)),
            };
            quote! {
                #i => {
                    ::miniconf::Deserialize::deserialize(de)
                        .map_err(|e| ::miniconf::Error::Inner(e))
                        .and_then(#validate)
                        .and_then(|v| {
                            self.#ident = v;
                            Ok(0)
                        })
                }
            }
        }
    });

    let depth = fields.iter().fold(0usize, |d, field| d.max(field.depth())) + 1;
    let ident = input.ident;

    let orig_generics = input.generics.clone();
    let (_, ty_generics, where_clause) = orig_generics.split_for_impl();
    let lts: Vec<_> = input.generics.lifetimes().cloned().collect();
    input.generics.params.push(parse_quote!('de));
    if let Some(syn::GenericParam::Lifetime(de)) = input.generics.params.last_mut() {
        assert_eq!(de.lifetime.ident, "de");
        for l in lts {
            assert!(l.lifetime.ident != "de");
            de.bounds.push(l.lifetime);
        }
    }
    let (impl_generics, _, _) = input.generics.split_for_impl();

    quote! {
        impl #impl_generics ::miniconf::TreeDeserialize<'de, #depth> for #ident #ty_generics #where_clause {
            fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, ::miniconf::Error<D::Error>>
            where
                K: Iterator,
                K::Item: ::miniconf::Key,
                D: ::miniconf::Deserializer<'de>,
            {
                let key = keys.next()
                    .ok_or(::miniconf::Error::TooShort(0))?;
                let index = ::miniconf::Key::find::<#depth, Self>(&key)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(::miniconf::Error::NotFound(1))?;
                if !defer && keys.next().is_some() {
                    return Err(::miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                ::miniconf::Increment::increment(match index {
                    #(#deserialize_by_key_arms ,)*
                    _ => unreachable!()
                })
            }
        }
    }.into()
}

/// Shorthand to derive the `TreeKey`, `TreeSerialize`, and `TreeDeserialize` traits for a struct.
#[proc_macro_derive(Tree, attributes(tree))]
pub fn derive_tree(input: TokenStream) -> TokenStream {
    let mut t = derive_tree_key(input.clone());
    t.extend(derive_tree_serialize(input.clone()));
    t.extend(derive_tree_deserialize(input));
    t
}
