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

fn get_path_arm(struct_field: &StructField) -> proc_macro2::TokenStream {
    // Quote context is a match of the field name with `self`, `path_parts`, and `value` available.
    let match_name = &struct_field.field.ident;
    if struct_field.deferred {
        quote! {
            stringify!(#match_name) => {
                self.#match_name.get_path(path_parts, ser)
            }
        }
    } else {
        quote! {
            stringify!(#match_name) => {
                if path_parts.next().is_some() {
                    Err(miniconf::Error::PathTooLong)
                } else {
                    Ok(miniconf::serde::ser::Serialize::serialize(&self.#match_name, ser)?)
                }
            }
        }
    }
}

fn set_path_arm(struct_field: &StructField) -> proc_macro2::TokenStream {
    // Quote context is a match of the field name with `self`, `path_parts`, and `value` available.
    let match_name = &struct_field.field.ident;
    if struct_field.deferred {
        quote! {
            stringify!(#match_name) => {
                self.#match_name.set_path(path_parts, de)
            }
        }
    } else {
        quote! {
            stringify!(#match_name) => {
                if path_parts.next().is_some() {
                    Err(miniconf::Error::PathTooLong)
                } else {
                    self.#match_name = miniconf::serde::de::Deserialize::deserialize(de)?;
                    Ok(())
                }
            }
        }
    }
}

fn next_path_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index with `self`, `state`, and `path` available.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.deferred {
        quote! {
            Some(#i) => {
                path.write_str(concat!("/", stringify!(#field_name)))
                    .map_err(|_| miniconf::IterError::Length)?;
                <#field_type>::next_path(state, depth + 1, path, separator)
            }
        }
    } else {
        quote! {
            Some(#i) => {
                path.write_str(concat!("/", stringify!(#field_name)))
                    .map_err(|_| miniconf::IterError::Length)?;
                Ok(depth)
            }
        }
    }
}

fn metadata_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.deferred {
        quote! {
            #i => {
                let mut meta = <#field_type>::metadata(separator_length);
                meta.max_length += separator_length + stringify!(#field_name).len();
                meta.max_depth += 1;
                meta
            }
        }
    } else {
        quote! {
            #i => {
                let mut meta = miniconf::Metadata::default();
                meta.max_length = separator_length + stringify!(#field_name).len();
                meta.max_depth = 1;
                meta.count = 1;
                meta
            }
        }
    }
}

fn name_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index with `self`, `state`, and `path` available.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.deferred {
        quote! {
            Some(#i) => {
                if full {
                    name.write_str(separator)
                        .and_then(|_| name.write_str(stringify!(#field_name)))?;
                }
                let r = <#field_type>::name(index, name, separator, full);
                <miniconf::graph::Result as miniconf::graph::Up>::up(r)
            }
        }
    } else {
        quote! {
            Some(#i) => {
                name.write_str(separator)
                    .and_then(|_| name.write_str(stringify!(#field_name)))?;
                Ok(miniconf::graph::Ok::Leaf(1))
            }
        }
    }
}

fn index_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index with `self`, `state`, and `path` available.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.deferred {
        quote! {
            Some(stringify!(#field_name)) => {
                index[0] = #i;
                let r = <#field_type>::index(path, &mut index[1..]);
                <miniconf::graph::Result as miniconf::graph::Up>::up(r)
            }
        }
    } else {
        quote! {
            Some(stringify!(#field_name)) => {
                index[0] = #i;
                Ok(miniconf::graph::Ok::Leaf(1))
            }
        }
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
        _ => unimplemented!("Only named fields are supported in structs."),
    };
    fields.iter().for_each(|f| f.bound_generics(generics));

    let set_path_arms = fields.iter().map(set_path_arm);
    let get_path_arms = fields.iter().map(get_path_arm);
    let next_path_arms = fields.iter().enumerate().map(next_path_arm);
    let metadata_arms = fields.iter().enumerate().map(metadata_arm);
    let name_arms = fields.iter().enumerate().map(name_arm);
    let index_arms = fields.iter().enumerate().map(index_arm);

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics miniconf::Miniconf for #ident #ty_generics #where_clause {
            fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), miniconf::Error<D::Error>>
            where
                P: Iterator<Item = &'a str>,
                D: miniconf::serde::Deserializer<'b>,
            {
                match path_parts.next().ok_or(miniconf::Error::PathTooShort)? {
                    #(#set_path_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound),
                }
            }

            fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, miniconf::Error<S::Error>>
            where
                P: Iterator<Item = &'a str>,
                S: miniconf::serde::Serializer,
            {
                match path_parts.next().ok_or(miniconf::Error::PathTooShort)? {
                    #(#get_path_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn next_path(
                state: &[usize],
                depth: usize,
                mut path: impl core::fmt::Write,
                separator: char,
            ) -> Result<usize, miniconf::IterError> {
                match state.get(depth).copied() {
                    #(#next_path_arms ,)*
                    Some(_) => Err(miniconf::IterError::Next(depth)),
                    None => Err(miniconf::IterError::Depth),
                }
            }

            fn metadata(separator_length: usize) -> miniconf::Metadata {
                let mut meta = miniconf::Metadata::default();

                for index in 0.. {
                    let item_meta: miniconf::Metadata = match index {
                        #(#metadata_arms ,)*
                        _ => break,
                    };

                    // Note(unreachable) Empty structs break immediatly
                    #[allow(unreachable_code)]
                    {
                        meta.max_length = meta.max_length.max(item_meta.max_length);
                        meta.max_depth = meta.max_depth.max(item_meta.max_depth);
                        meta.count += item_meta.count;
                    }
                }

                meta
            }
        }

        impl #impl_generics miniconf::graph::Graph for #ident #ty_generics #where_clause {
            fn name<I: Iterator<Item = usize>, N: core::fmt::Write>(
                index: &mut I,
                name: &mut N,
                separator: &str,
                full: bool,
            ) -> miniconf::graph::Result {
                match index.next() {
                    None => Ok(miniconf::graph::Ok::Internal(0)),
                    #(#name_arms ,)*
                    _ => Err(miniconf::graph::Error::NotFound(0)),
                }
            }

            fn index<'a, P: Iterator<Item = &'a str>>(path: &mut P, index: &mut [usize]) -> miniconf::graph::Result {
                match path.next() {
                    None => Ok(miniconf::graph::Ok::Internal(0)),
                    _ if index.is_empty() => Err(miniconf::graph::Error::TooShort),
                    #(#index_arms ,)*
                    _ => Err(miniconf::graph::Error::NotFound(0)),
                }
            }
        }
    }
    .into()
}
