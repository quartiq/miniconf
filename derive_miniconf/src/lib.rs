use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod attributes;
mod field;

use field::StructField;

/// Represents a type definition with associated generics.
pub(crate) struct TypeDefinition {
    pub generics: syn::Generics,
    pub name: syn::Ident,
}

impl TypeDefinition {
    pub fn new(generics: syn::Generics, name: syn::Ident) -> Self {
        Self { generics, name }
    }
}

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

    let typedef = TypeDefinition::new(input.generics, input.ident);

    match input.data {
        syn::Data::Struct(struct_data) => derive_struct(typedef, struct_data),
        _ => unimplemented!(),
    }
}

/// Derive the Miniconf trait for structs.
///
/// # Args
/// * `typedef` - The type definition.
/// * `data` - The data associated with the struct definition.
/// * `atomic` - specified true if the data must be updated atomically. If false, data must be
///   set at a terminal node.
///
/// # Returns
/// A token stream of the generated code.
fn derive_struct(mut typedef: TypeDefinition, data: syn::DataStruct) -> TokenStream {
    let raw_fields = match data.fields {
        syn::Fields::Named(syn::FieldsNamed { ref named, .. }) => named,
        _ => unimplemented!("Only named fields are supported in structs."),
    };

    let fields: Vec<StructField> = raw_fields
        .iter()
        .map(|x| StructField::new(x.clone()))
        .collect();

    for field in fields.iter() {
        field.bound_generics(&mut typedef);
    }

    let set_path_arms = fields.iter().map(|f| {
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
                    if path_parts.peek().is_some() {
                        return Err(miniconf::Error::PathTooLong)
                    }

                    self.#match_name = miniconf::serde_json_core::from_slice(value)?.0;
                    Ok(())
                }
            }
        }
    });

    let get_path_arms = fields.iter().map(|f| {
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
                    if path_parts.peek().is_some() {
                        return Err(miniconf::Error::PathTooLong);
                    }

                    miniconf::serde_json_core::to_slice(&self.#match_name, value).map_err(|_| miniconf::Error::SerializationFailed)
                }
            }
        }
    });

    let next_path_arms = fields.iter().enumerate().map(|(i, f)| {
        let field_name = &f.field.ident;
        if f.deferred {
            quote! {
                #i => {
                    let original_length = path.len();

                    if path.push_str(concat!(stringify!(#field_name), "/")).is_err() {
                        // Note: During expected execution paths using `into_iter()`, the size of the
                        // topic buffer is checked in advance to make sure this condition doesn't
                        // occur.  However, it's possible to happen if the user manually calls
                        // `next_path`.
                        unreachable!("Topic buffer too short");
                    }

                    if self.#field_name.next_path(&mut state[1..], path) {
                        return true;
                    }

                    // Strip off the previously prepended index, since we completed that element and need
                    // to instead check the next one.
                    path.truncate(original_length);

                    state[0] += 1;
                    state[1..].iter_mut().for_each(|x| *x = 0);
                }
            }
        } else {
            quote! {
                #i => {
                    if path.push_str(stringify!(#field_name)).is_err() {
                        // Note: During expected execution paths using `into_iter()`, the size of the
                        // topic buffer is checked in advance to make sure this condition doesn't
                        // occur.  However, it's possible to happen if the user manually calls
                        // `next_path`.
                        unreachable!("Topic buffer too short");
                    }
                    state[0] += 1;

                    return true;
                }
            }
        }
    });

    let metadata_arms = fields.iter().enumerate().map(|(i, f)| {
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

                    meta
                }
            }
        }
    });

    let (impl_generics, ty_generics, where_clause) = typedef.generics.split_for_impl();
    let name = typedef.name;

    let expanded = quote! {
        impl #impl_generics miniconf::Miniconf for #name #ty_generics #where_clause {
            fn set_path<'a, P: miniconf::Peekable<Item = &'a str>>(
                &mut self,
                path_parts: &'a mut P,
                value: &[u8]
            ) -> Result<(), miniconf::Error> {
                let field = path_parts.next().ok_or(miniconf::Error::PathTooShort)?;

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

                match field {
                    #(#get_path_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn metadata() -> miniconf::Metadata {
                // Loop through all child elements, collecting the maximum length + depth of any
                // member.
                let mut meta = miniconf::Metadata::default();

                for index in 0.. {
                    let item_meta = match index {
                        #(#metadata_arms ,)*
                        _ => break,
                    };

                    meta.max_length = meta.max_length.max(item_meta.max_length);
                    meta.max_depth = meta.max_depth.max(item_meta.max_depth);
                }

                meta
            }

            fn next_path<const TS: usize>(&self, state: &mut [usize], path: &mut miniconf::heapless::String<TS>) -> bool {
                if state.len() == 0 {
                    // Note: During expected execution paths using `into_iter()`, the size of the
                    // state stack is checked in advance to make sure this condition doesn't occur.
                    // However, it's possible to happen if the user manually calls `next_path`.
                    unreachable!("State stack too small");
                }

                loop {
                    match state[0] {
                        #(#next_path_arms ,)*
                        _ => return false,

                    };
                }
            }
        }
    };

    TokenStream::from(expanded)
}
