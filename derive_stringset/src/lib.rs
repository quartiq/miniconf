use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Derive the StringSet trait for custom types.
///
/// # Args
/// * `input` - The input token stream for the proc-macro.
///
/// # Returns
/// A token stream of the generated code.
#[proc_macro_derive(StringSet)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident.clone();
    match input.data {
        syn::Data::Struct(struct_data) => derive_struct(name, struct_data),
        syn::Data::Enum(enum_data) => derive_enum(name, enum_data),
        syn::Data::Union(_) => unimplemented!(),
    }
}

/// Derive the StringSet trait for structs.
///
/// # Args
/// * `name` - The name of the enum
/// * `data` - The data associated with the struct definition.
///
/// # Returns
/// A token stream of the generated code.
fn derive_struct(name: syn::Ident, data: syn::DataStruct) -> TokenStream {
    let fields = match data.fields {
        syn::Fields::Named(syn::FieldsNamed { ref named, .. }) => named,
        _ => unimplemented!("Only named fields are supported in structs."),
    };

    let recurse_match_arms = fields.iter().map(|f| {
        let match_name = &f.ident;
        quote! {
            stringify!(#match_name) => {
                self.#match_name.string_set(topic_parts, value)?;
                Ok(())
            }
        }
    });

    let direct_set_match_arms = fields.iter().map(|f| {
        let match_name = &f.ident;
        quote! {
            stringify!(#match_name) => {
                self.#match_name = miniconf::serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    });

    let expanded = quote! {
        impl StringSet for #name {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                let field = topic_parts.next().ok_or(miniconf::Error::NameTooShort)?;
                let next = topic_parts.peek();

                if next.is_some() {
                    match field {
                        #(#recurse_match_arms ,)*
                        _ => Err(miniconf::Error::NameNotFound)
                    }
                } else {
                    match field {
                        #(#direct_set_match_arms ,)*
                        _ => Err(miniconf::Error::NameNotFound)
                    }
                }
            }

        }

    };

    TokenStream::from(expanded)
}

/// Derive the StringSet trait for simple enums.
///
/// # Args
/// * `name` - The name of the enum
/// * `data` - The data associated with the enum definition.
///
/// # Returns
/// A token stream of the generated code.
fn derive_enum(name: syn::Ident, data: syn::DataEnum) -> TokenStream {
    // Only support simple enums, check each field
    for v in data.variants.iter() {
        match v.fields {
            syn::Fields::Named(_) | syn::Fields::Unnamed(_) => {
                unimplemented!("Only simple, C-like enums are supported.")
            }
            syn::Fields::Unit => {}
        }
    }

    let expanded = quote! {
        impl miniconf::StringSet for #name {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                if topic_parts.peek().is_some() {
                    // We don't support enums that can contain other values
                    return Err(miniconf::Error::NameTooLong)
                }

                *self = miniconf::serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    };

    TokenStream::from(expanded)
}
