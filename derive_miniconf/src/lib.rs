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
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        syn::Data::Struct(_) => derive_struct(input),
        _ => unimplemented!(),
    }
}

fn get_path_arm(f: &StructField) -> proc_macro2::TokenStream {
    let match_name = &f.field.ident;
    if f.deferred {
        quote! {
            stringify!(#match_name) => {
                self.#match_name.get_path(path_parts, value)
            }
        }
    } else {
        quote! {
            stringify!(#match_name) => {
                if peek {
                    return Err(miniconf::Error::PathTooLong);
                } else {
                    Ok(miniconf::serde_json_core::to_slice(&self.#match_name, value)?)
                }
            }
        }
    }
}

fn set_path_arm(f: &StructField) -> proc_macro2::TokenStream {
    let match_name = &f.field.ident;
    if f.deferred {
        quote! {
            stringify!(#match_name) => {
                self.#match_name.set_path(path_parts, value)
            }
        }
    } else {
        quote! {
            stringify!(#match_name) => {
                if peek {
                    Err(miniconf::Error::PathTooLong)
                } else {
                    let (value, len) = miniconf::serde_json_core::from_slice(value)?;
                    self.#match_name = value;
                    Ok(len)
                }
            }
        }
    }
}

fn next_path_arm((i, f): (usize, &StructField)) -> proc_macro2::TokenStream {
    let field_type = &f.field.ty;
    let field_name = &f.field.ident;
    if f.deferred {
        quote! {
            #i => {
                path.push_str(concat!(stringify!(#field_name), "/"))
                    .map_err(|_| miniconf::IterError::PathLength)?;

                if <#field_type>::next_path(&mut state[1..], path)? {
                    return Ok(true);
                }
            }
        }
    } else {
        quote! {
            #i => {
                path.push_str(stringify!(#field_name))
                    .map_err(|_| miniconf::IterError::PathLength)?;
                state[0] += 1;

                return Ok(true);
            }
        }
    }
}

fn metadata_arm((i, f): (usize, &StructField)) -> proc_macro2::TokenStream {
    let field_type = &f.field.ty;
    let field_name = &f.field.ident;
    if f.deferred {
        quote! {
            #i => {
                let mut meta = <#field_type>::metadata();

                // Unconditionally account for separator since we add it
                // even if elements that are deferred to (`Options`)
                // may have no further hierarchy to add and remove the separator again.
                meta.max_length += concat!(stringify!(#field_name), "/").len();
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

/// Derive the Miniconf trait for structs.
///
/// # Args
/// * `input` - The derive input token stream.
/// * `data` - The data associated with the struct definition.
/// * `atomic` - specified true if the data must be updated atomically. If false, data must be
///   set at a terminal node.
///
/// # Returns
/// A token stream of the generated code.
fn derive_struct(mut input: DeriveInput) -> TokenStream {
    let data = match &input.data {
        syn::Data::Struct(data) => data,
        _ => unimplemented!(),
    };
    let fields: Vec<_> = match &data.fields {
        syn::Fields::Named(syn::FieldsNamed { named, .. }) => {
            named.iter().cloned().map(StructField::new).collect()
        }
        _ => unimplemented!("Only named fields are supported in structs."),
    };
    fields
        .iter()
        .for_each(|f| f.bound_generics(&mut input.generics));

    let set_path_arms = fields.iter().map(set_path_arm);
    let get_path_arms = fields.iter().map(get_path_arm);
    let next_path_arms = fields.iter().enumerate().map(next_path_arm);
    let metadata_arms = fields.iter().enumerate().map(metadata_arm);

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let name = input.ident;

    quote! {
        impl #impl_generics miniconf::Miniconf for #name #ty_generics #where_clause {
            fn set_path<'a, P: miniconf::Peekable<Item = &'a str>>(
                &mut self,
                path_parts: &'a mut P,
                value: &[u8]
            ) -> Result<usize, miniconf::Error> {
                let field = path_parts.next().ok_or(miniconf::Error::PathTooShort)?;
                let peek = path_parts.peek().is_some();

                match field {
                    #(#set_path_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn get_path<'a, P: miniconf::Peekable<Item = &'a str>>(
                &self,
                path_parts: &'a mut P,
                value: &mut [u8]
            ) -> Result<usize, miniconf::Error> {
                let field = path_parts.next().ok_or(miniconf::Error::PathTooShort)?;
                let peek = path_parts.peek().is_some();

                match field {
                    #(#get_path_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn next_path<const TS: usize>(
                state: &mut [usize],
                path: &mut miniconf::heapless::String<TS>
            ) -> Result<bool, miniconf::IterError> {
                let original_length = path.len();
                loop {
                    match *state.first().ok_or(miniconf::IterError::PathDepth)? {
                        #(#next_path_arms ,)*
                        _ => return Ok(false),
                    };

                    // Note(unreachable) Without any deferred fields, every arm above returns
                    #[allow(unreachable_code)]
                    {
                        // If a deferred field is done, strip off the field name again,
                        // and advance to the next field.
                        path.truncate(original_length);

                        state[0] += 1;
                        state[1..].fill(0);
                    }
                }
            }

            fn metadata() -> miniconf::Metadata {
                // Loop through all child elements, collecting the maximum length + depth of any
                // member.
                let mut meta = miniconf::Metadata::default();

                for index in 0.. {
                    let item_meta: miniconf::Metadata = match index {
                        #(#metadata_arms ,)*
                        _ => break,
                    };

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
