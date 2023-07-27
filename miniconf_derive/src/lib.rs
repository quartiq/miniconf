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

fn get_by_name_arm(struct_field: &StructField) -> proc_macro2::TokenStream {
    // Quote context is a match of the field name with `get_by_name()` args available.
    let field_name = &struct_field.field.ident;
    if struct_field.defer {
        quote! {
            stringify!(#field_name) => {
                let r = self.#field_name.get_by_name(names, ser);
                miniconf::Increment::increment(r)
            }
        }
    } else {
        quote! {
            stringify!(#field_name) => {
                if names.next().is_some() {
                    Err(miniconf::Error::TooLong(1))
                } else {
                    miniconf::serde::ser::Serialize::serialize(&self.#field_name, ser)?;
                    Ok(miniconf::Ok::Leaf(1))
                }
            }
        }
    }
}

fn set_by_name_arm(struct_field: &StructField) -> proc_macro2::TokenStream {
    // Quote context is a match of the field name with `set_by_name()` args available.
    let field_name = &struct_field.field.ident;
    if struct_field.defer {
        quote! {
            stringify!(#field_name) => {
                let r = self.#field_name.set_by_name(names, de);
                miniconf::Increment::increment(r)
            }
        }
    } else {
        quote! {
            stringify!(#field_name) => {
                if names.next().is_some() {
                    Err(miniconf::Error::TooLong(1))
                } else {
                    self.#field_name = miniconf::serde::de::Deserialize::deserialize(de)?;
                    Ok(miniconf::Ok::Leaf(1))
                }
            }
        }
    }
}

fn metadata_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index with `metadata()` args available.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.defer {
        quote! {
            #i => {
                let mut meta = <#field_type>::metadata();
                meta.max_length += stringify!(#field_name).len();
                meta.max_depth += 1;
                meta
            }
        }
    } else {
        quote! {
            #i => {
                let mut meta = miniconf::Metadata::default();
                meta.max_length = stringify!(#field_name).len();
                meta.max_depth = 1;
                meta.count = 1;
                meta
            }
        }
    }
}

fn traverse_by_index_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index with `traverse_by_index()` args available.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.defer {
        quote! {
            #i => {
                func(miniconf::Ok::Internal(1), #i, stringify!(#field_name)).map_err(|e| miniconf::Error::Inner(e))?;
                let r = <#field_type>::traverse_by_index(indices, func);
                miniconf::Increment::increment(r)
            }
        }
    } else {
        quote! {
            #i => {
                func(miniconf::Ok::Leaf(1), #i, stringify!(#field_name)).map_err(|e| miniconf::Error::Inner(e))?;
                Ok(miniconf::Ok::Leaf(1))
            }
        }
    }
}

fn traverse_by_name_arm((i, struct_field): (usize, &StructField)) -> proc_macro2::TokenStream {
    // Quote context is a match of the field index with `traverse_by_name()` args available.
    let field_type = &struct_field.field.ty;
    let field_name = &struct_field.field.ident;
    if struct_field.defer {
        quote! {
            stringify!(#field_name) => {
                func(miniconf::Ok::Internal(1), #i, stringify!(#field_name)).map_err(|e| miniconf::Error::Inner(e))?;
                let r = <#field_type>::traverse_by_name(names, func);
                miniconf::Increment::increment(r)
            }
        }
    } else {
        quote! {
            stringify!(#field_name) => {
                func(miniconf::Ok::Leaf(1), #i, stringify!(#field_name)).map_err(|e| miniconf::Error::Inner(e))?;
                Ok(miniconf::Ok::Leaf(1))
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

    let set_by_name_arms = fields.iter().map(set_by_name_arm);
    let get_by_name_arms = fields.iter().map(get_by_name_arm);
    let metadata_arms = fields.iter().enumerate().map(metadata_arm);
    let traverse_by_index_arms = fields.iter().enumerate().map(traverse_by_index_arm);
    let traverse_by_name_arms = fields.iter().enumerate().map(traverse_by_name_arm);

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics miniconf::Miniconf for #ident #ty_generics #where_clause {
            fn set_by_name<'a, 'b, P, D>(&mut self, names: &mut P, de: D) -> miniconf::Result<D::Error>
            where
                P: Iterator<Item = &'a str>,
                D: miniconf::serde::Deserializer<'b>,
            {
                match names.next().ok_or(miniconf::Error::Internal(0))? {
                    #(#set_by_name_arms ,)*
                    _ => Err(miniconf::Error::NotFound(1)),
                }
            }

            fn get_by_name<'a, P, S>(&self, names: &mut P, ser: S) -> miniconf::Result<S::Error>
            where
                P: Iterator<Item = &'a str>,
                S: miniconf::serde::Serializer,
            {
                match names.next().ok_or(miniconf::Error::Internal(0))? {
                    #(#get_by_name_arms ,)*
                    _ => Err(miniconf::Error::NotFound(1))
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

            fn traverse_by_index<P, F, E>(
                indices: &mut P,
                mut func: F,
            ) -> miniconf::Result<E>
            where
                P: Iterator<Item = usize>,
                F: FnMut(miniconf::Ok, usize, &str) -> Result<(), E>,
            {
                match indices.next() {
                    None => Ok(miniconf::Ok::Internal(0)),
                    Some(index) => match index {
                        #(#traverse_by_index_arms ,)*
                        _ => Err(miniconf::Error::NotFound(1)),
                    }
                }
            }

            fn traverse_by_name<'a, P, F, E>(
                names: &mut P,
                mut func: F,
            ) -> miniconf::Result<E>
            where
                P: Iterator<Item = &'a str>,
                F: FnMut(miniconf::Ok, usize, &str) -> Result<(), E>,
            {
                match names.next() {
                    None => Ok(miniconf::Ok::Internal(0)),
                    Some(name) => match name {
                        #(#traverse_by_name_arms ,)*
                        _ => Err(miniconf::Error::NotFound(1)),
                    }
                }
            }
        }
    }
    .into()
}
