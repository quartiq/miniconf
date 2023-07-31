use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod field;

use field::StructField;

/// Derive the Miniconf trait for custom types.
///
/// Each field of the struct will be recursively used to construct a unique path for all elements.
///
/// All paths are similar to file-system paths with variable names separated by forward
/// slashes.
///
/// For arrays, the array index is treated as a unique identifier. That is, to access the first
/// element of array `data`, the path would be `data/0`.
///
/// # Example
/// ```rust
/// #[derive(Miniconf)]
/// struct Nested {
///     #[miniconf(defer)]
///     data: [u32; 2],
/// }
/// #[derive(Miniconf)]
/// struct Settings {
///     // Accessed with path `nested/data/0` or `nested/data/1`
///     #[miniconf(defer)]
///     nested: Nested,
///
///     // Accessed with path `external`
///     external: bool,
/// }
#[proc_macro_derive(Miniconf, attributes(miniconf))]
pub fn derive(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);

    match input.data {
        syn::Data::Struct(ref data) => derive_struct(data, &mut input.generics, &input.ident),
        _ => unimplemented!(),
    }
}

fn name(i: usize, ident: &Option<syn::Ident>) -> proc_macro2::TokenStream {
    match ident {
        None => {
            let index = syn::Index::from(i);
            quote! { #index }
        }
        Some(name) => quote! { #name },
    }
}

fn get_by_key_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field name with `get_by_key()` args available.
    let ident = name(i, &struct_field.field.ident);
    if let Some(depth) = struct_field.defer {
        quote! {
            #i => miniconf::Miniconf::<#depth>::get_by_key(&self.#ident, keys, ser)
        }
    } else {
        quote! {
            #i => {
                miniconf::serde::Serialize::serialize(&self.#ident, ser)?;
                Ok(0)
           }
        }
    }
}

fn set_by_key_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field name with `set_by_key()` args available.
    let ident = name(i, &struct_field.field.ident);
    if let Some(depth) = struct_field.defer {
        quote! {
            #i => miniconf::Miniconf::<#depth>::set_by_key(&mut self.#ident, keys, de)
        }
    } else {
        quote! {
            #i => {
                self.#ident = miniconf::serde::Deserialize::deserialize(de)?;
                Ok(0)
            }
        }
    }
}

fn traverse_by_key_arm(
    (i, struct_field): (usize, &StructField),
) -> Option<proc_macro2::TokenStream> {
    // Quote context is a match of the field index with `traverse_by_key()` args available.
    if let Some(depth) = struct_field.defer {
        let field_type = &struct_field.field.ty;
        Some(quote! {
            #i => <#field_type as miniconf::Miniconf<#depth>>::traverse_by_key(keys, func)
        })
    } else {
        None
    }
}

fn metadata_arm((i, struct_field): (usize, &StructField)) -> Option<proc_macro2::TokenStream> {
    // Quote context is a match of the field index with `metadata()` args available.
    if let Some(depth) = struct_field.defer {
        let field_type = &struct_field.field.ty;
        Some(quote! {
            #i => <#field_type as miniconf::Miniconf<#depth>>::metadata()
        })
    } else {
        None
    }
}

/// Derive the Miniconf trait for structs.
///
/// # Args
/// * `data` - The data associated with the struct definition.
/// * `generics` - The generics of the definition. Sufficient bounds will be added here.
/// * `ident` - The identifier to derive the impl for.
///
/// # Returns
/// A token stream of the generated code.
fn derive_struct(
    data: &syn::DataStruct,
    generics: &mut syn::Generics,
    ident: &syn::Ident,
) -> TokenStream {
    let fields: Vec<_> = match &data.fields {
        syn::Fields::Named(syn::FieldsNamed { named, .. }) => {
            named.iter().cloned().map(StructField::new).collect()
        }
        syn::Fields::Unnamed(syn::FieldsUnnamed { unnamed, .. }) => {
            unnamed.iter().cloned().map(StructField::new).collect()
        }
        syn::Fields::Unit => unimplemented!("Unit struct not supported"),
    };
    let orig_generics = generics.clone();
    fields.iter().for_each(|f| f.bound_generics(generics));

    let get_by_key_arms = fields.iter().enumerate().map(get_by_key_arm);
    let set_by_key_arms = fields.iter().enumerate().map(set_by_key_arm);
    let traverse_by_key_arms = fields.iter().enumerate().filter_map(traverse_by_key_arm);
    let metadata_arms = fields.iter().enumerate().filter_map(metadata_arm);
    let names = fields.iter().enumerate().map(|(i, field)| {
        let name = name(i, &field.field.ident);
        quote! { stringify!(#name) }
    });
    let fields_len = fields.len();

    let defers = fields.iter().map(|field| {
        if let Some(depth) = field.defer {
            quote! {Some(#depth)}
        } else {
            quote! {None}
        }
    });

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let (impl_generics_orig, ty_generics_orig, _where_clause_orig) = orig_generics.split_for_impl();

    let tokens = quote! {
        impl #impl_generics_orig #ident #ty_generics_orig {
            const __MINICONF_NAMES: [&str; #fields_len] = [#(#names ,)*];
            const __MINICONF_DEFERS: [Option<usize>; #fields_len] = [#(#defers ,)*];
        }

        impl #impl_generics miniconf::Miniconf<1> for #ident #ty_generics #where_clause {
            fn name_to_index(value: &str) -> Option<usize> {
                Self::__MINICONF_NAMES.iter().position(|&n| n == value)
            }

            fn get_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, miniconf::Error<S::Error>>
            where
                K: Iterator,
                K::Item: miniconf::Key,
                S: miniconf::serde::Serializer,
            {
                let key = keys.next()
                    .ok_or(miniconf::Error::TooShort(0))?;
                let index = miniconf::Key::find::<1, Self>(&key)
                    .ok_or(miniconf::Error::NotFound(1))?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(miniconf::Error::NotFound(1))?;
                if !defer.is_some() && keys.next().is_some() {
                    return Err(miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                miniconf::Increment::increment(match index {
                    #(#get_by_key_arms ,)*
                    _ => unreachable!()
                })
            }

            fn set_by_key<'a, K, D>(&mut self, mut keys: K, de: D) -> Result<usize, miniconf::Error<D::Error>>
            where
                K: Iterator,
                K::Item: miniconf::Key,
                D: miniconf::serde::Deserializer<'a>,
            {
                let key = keys.next()
                    .ok_or(miniconf::Error::TooShort(0))?;
                let index = miniconf::Key::find::<1, Self>(&key)
                    .ok_or(miniconf::Error::NotFound(1))?;
                let defer = Self::__MINICONF_DEFERS.get(index)
                    .ok_or(miniconf::Error::NotFound(1))?;
                if !defer.is_some() && keys.next().is_some() {
                    return Err(miniconf::Error::TooLong(1))
                }
                // Note(unreachable) empty structs have diverged by now
                #[allow(unreachable_code)]
                miniconf::Increment::increment(match index {
                    #(#set_by_key_arms ,)*
                    _ => unreachable!()
                })
            }

            fn traverse_by_key<K, F, E>(
                mut keys: K,
                mut func: F,
            ) -> Result<usize, miniconf::Error<E>>
            where
                K: Iterator,
                K::Item: miniconf::Key,
                F: FnMut(usize, &str) -> Result<(), E>,
            {
                let key = keys.next()
                    .ok_or(miniconf::Error::TooShort(0))?;
                let index = miniconf::Key::find::<1, Self>(&key)
                    .ok_or(miniconf::Error::NotFound(1))?;
                let name = Self::__MINICONF_NAMES.get(index)
                    .ok_or(miniconf::Error::NotFound(1))?;
                func(index, name)?;
                miniconf::Increment::increment(match index {
                    #(#traverse_by_key_arms ,)*
                    _ => Ok(0),
                })
            }

            fn metadata() -> miniconf::Metadata {
                let mut meta = miniconf::Metadata::default();
                for index in 0..#fields_len {
                    let item_meta: miniconf::Metadata = match index {
                        #(#metadata_arms ,)*
                        _ => {
                            let mut m = miniconf::Metadata::default();
                            m.count = 1;
                            m
                        }
                    };
                    meta.max_length = meta.max_length.max(
                        Self::__MINICONF_NAMES[index].len() +
                        item_meta.max_length
                    );
                    meta.max_depth = meta.max_depth.max(
                        1 +
                        item_meta.max_depth
                    );
                    meta.count += item_meta.count;
                }
                meta
            }
        }
    }
    .into();
    // eprintln!("{}", tokens);
    tokens
}
