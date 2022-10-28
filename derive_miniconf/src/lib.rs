use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

mod attributes;
mod field;

use field::StructField;

/// Represents a type definition with associated generics.
struct TypeDefinition {
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
/// All settings paths are similar to file-system paths with variable names separated by forward
/// slashes.
///
/// For arrays, the array index is treated as a unique identifier. That is, to access the first
/// element of array `test`, the path would be `test/0`.
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

    let fields: Vec<StructField> = raw_fields.iter().map(|x| StructField::new(x.clone())).collect();

    for field in fields.iter() {
        field.bound_generics(&mut typedef);
    }

    let set_recurse_match_arms = fields.iter().map(|f| {
        let match_name = &f.field.ident;
        if f.deferred {
            quote! {
                stringify!(#match_name) => {
                    self.#match_name.string_set(topic_parts, value)
                }
            }
        } else {
            quote! {
                stringify!(#match_name) => {
                    if topic_parts.peek().is_some() {
                        return Err(miniconf::Error::PathTooLong)
                    }

                    self.#match_name = miniconf::serde_json_core::from_slice(value)?.0;
                    Ok(())
                }
            }
        }
    });

    let get_recurse_match_arms = fields.iter().map(|f| {
        let match_name = &f.field.ident;
        if f.deferred {
            quote! {
                stringify!(#match_name) => {
                    self.#match_name.string_get(topic_parts, value)
                }
            }
        } else {
            quote! {
                stringify!(#match_name) => {
                    if topic_parts.peek().is_some() {
                        return Err(miniconf::Error::PathTooLong);
                    }

                    miniconf::serde_json_core::to_slice(&self.#match_name, value).map_err(|_| miniconf::Error::SerializationFailed)
                }
            }
        }
    });

    let iter_match_arms = fields.iter().enumerate().map(|(i, f)| {
        let field_name = &f.field.ident;
        if f.deferred {
            quote! {
                #i => {
                    let original_length = topic.len();

                    let postfix = if topic.len() != 0 {
                        concat!("/", stringify!(#field_name))
                    } else {
                        stringify!(#field_name)
                    };

                    if topic.push_str(postfix).is_err() {
                        // Note: During expected execution paths using `into_iter()`, the size of the
                        // topic buffer is checked in advance to make sure this condition doesn't
                        // occur.  However, it's possible to happen if the user manually calls
                        // `recurse_paths`.
                        unreachable!("Topic buffer too short");
                    }

                    if self.#field_name.recurse_paths(&mut index[1..], topic).is_some() {
                        return Some(());
                    }

                    // Strip off the previously prepended index, since we completed that element and need
                    // to instead check the next one.
                    topic.truncate(original_length);

                    index[0] += 1;
                    index[1..].iter_mut().for_each(|x| *x = 0);
                }
            }
        } else {
            quote! {
                #i => {
                    let i = index[0];
                    index[0] += 1;

                    if i == 0 {
                        return Some(())
                    }
                }
            }
        }
    });

    let iter_metadata_arms = fields.iter().enumerate().map(|(i, f)| {
        let field_name = &f.field.ident;
        if f.deferred {
            quote! {
                #i => {
                    let mut meta = self.#field_name.get_metadata();

                    // If the subfield has additional paths, we need to add space for a separator.
                    if meta.max_topic_size > 0 {
                        meta.max_topic_size += 1;
                    }

                    meta.max_topic_size += stringify!(#field_name).len();

                    meta
                }
            }
        } else {
            quote! {
                #i => {
                    miniconf::MiniconfMetadata {
                        max_topic_size: stringify!(#field_name).len(),
                        max_depth: 1,
                    }
                }
            }
        }
    });

    let (impl_generics, ty_generics, where_clause) = typedef.generics.split_for_impl();
    let name = typedef.name;

    let expanded = quote! {
        impl #impl_generics miniconf::Miniconf for #name #ty_generics #where_clause {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                let field = topic_parts.next().ok_or(miniconf::Error::PathTooShort)?;

                match field {
                    #(#set_recurse_match_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn string_get(&self, mut topic_parts: core::iter::Peekable<core::str::Split<char>>, value: &mut [u8]) -> Result<usize, miniconf::Error> {
                let field = topic_parts.next().ok_or(miniconf::Error::PathTooShort)?;

                match field {
                    #(#get_recurse_match_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn get_metadata(&self) -> miniconf::MiniconfMetadata {
                // Loop through all child elements, collecting the maximum length + depth of any
                // member.
                let mut maximum_sizes = miniconf::MiniconfMetadata {
                    max_topic_size: 0,
                    max_depth: 0
                };

                let mut index = 0;
                loop {
                    let metadata = match index {
                        #(#iter_metadata_arms ,)*
                        _ => break,
                    };

                    maximum_sizes.max_topic_size = core::cmp::max(maximum_sizes.max_topic_size,
                                                                  metadata.max_topic_size);
                    maximum_sizes.max_depth = core::cmp::max(maximum_sizes.max_depth,
                                                             metadata.max_depth);

                    index += 1;
                }

                // We need an additional index depth for this node.
                maximum_sizes.max_depth += 1;

                maximum_sizes
            }

            fn recurse_paths<const TS: usize>(&self, index: &mut [usize], topic: &mut miniconf::heapless::String<TS>) -> Option<()> {
                if index.len() == 0 {
                    // Note: During expected execution paths using `into_iter()`, the size of the
                    // index stack is checked in advance to make sure this condition doesn't occur.
                    // However, it's possible to happen if the user manually calls `recurse_paths`.
                    unreachable!("Index stack too small");
                }

                loop {
                    match index[0] {
                        #(#iter_match_arms ,)*
                        _ => return None,

                    };
                }
            }
        }
    };

    TokenStream::from(expanded)
}
