use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(StringSet)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match &input.data {
        syn::Data::Struct(_) => derive_struct(&input),
        syn::Data::Enum(_) => derive_enum(&input),
        syn::Data::Union(_) => {
            unimplemented!()
        }
    }
}

fn derive_struct(input: &syn::DeriveInput) -> TokenStream {
    let fields = if let syn::Data::Struct(syn::DataStruct {
        fields: syn::Fields::Named(syn::FieldsNamed { ref named, .. }),
        ..
    }) = input.data
    {
        named
    } else {
        // A struct with named fields is the only supported input
        unimplemented!();
    };

    let recurse_match_arms = fields.iter().map(|f| {
        let name = &f.ident;
        quote! {
            stringify!(#name) => {
                self.#name.string_set(topic_parts, value)?;
                Ok(())
            }
        }
    });

    let direct_set_match_arms = fields.iter().map(|f| {
        let name = &f.ident;
        quote! {
            stringify!(#name) => {
                self.#name = miniconf::serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    });

    let name = &input.ident;
    let expanded = quote! {
        impl StringSet for #name {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                let field = topic_parts.next().ok_or(miniconf::Error::NameTooShort)?;
                let next = topic_parts.peek();

                if let Some(_next) = next {
                    match field {
                        #(#recurse_match_arms ,)*
                        _ => Err(miniconf::Error::NameNotFound)
                    }

                } else {
                    if topic_parts.peek().is_some() {
                        return Err(miniconf::Error::NameTooLong);
                    }
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

fn derive_enum(input: &syn::DeriveInput) -> TokenStream {
    let variants = if let syn::Data::Enum(syn::DataEnum { ref variants, .. }) = input.data {
        variants
    } else {
        // We should not have called derive_enum() if input.data wasn't an enum
        unreachable!();
    };

    // Only support simple enums, check each field
    for v in variants.iter() {
        match v.fields {
            syn::Fields::Named(_) | syn::Fields::Unnamed(_) => {
                unimplemented!("only simple enums are supported")
            }
            syn::Fields::Unit => {}
        }
    }

    let name = &input.ident;
    let expanded = quote! {
        impl StringSet for #name {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                if topic_parts.peek().is_some() {
                    // We don't net support enums that can contain other values
                    Err(miniconf::Error::NameTooLong)
                } else {
                    *self = miniconf::serde_json_core::from_slice(value)?.0;
                    Ok(())
                }
            }
        }
    };

    TokenStream::from(expanded)
}
