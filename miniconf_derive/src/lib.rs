use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod attributes;
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
                    serde::ser::Serialize::serialize(&self.#match_name, ser).map_err(|_| miniconf::Error::Serialization)
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
                    self.#match_name = serde::de::Deserialize::deserialize(de).map_err(|_| miniconf::Error::Deserialization)?;
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
                let mut meta = <#field_type>::metadata();
                meta.max_length += 1 + stringify!(#field_name).len();
                meta.max_depth += 1;
                meta
            }
        }
    } else {
        quote! {
            #i => {
                let mut meta = miniconf::Metadata::default();
                meta.max_length = 1 + stringify!(#field_name).len();
                meta.max_depth = 1;
                meta.count = 1;
                meta
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

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics miniconf::Miniconf for #ident #ty_generics #where_clause {
            fn set_path<'a, 'b: 'a, P, D>(&mut self, path_parts: &mut P, de: D) -> Result<(), miniconf::Error>
            where
                P: Iterator<Item = &'a str>,
                D: serde::Deserializer<'b>,
            {
                match path_parts.next().ok_or(miniconf::Error::PathTooShort)? {
                    #(#set_path_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound),
                }
            }

            fn get_path<'a, P, S>(&self, path_parts: &mut P, ser: S) -> Result<S::Ok, miniconf::Error>
            where
                P: Iterator<Item = &'a str>,
                S: serde::Serializer,
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

            fn metadata() -> miniconf::Metadata {
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
    }
    .into()
}
