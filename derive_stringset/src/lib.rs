use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(StringSet)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;
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
                self.#name = serde_json_core::from_slice(value)?.0;
                Ok(())
            }
        }
    });

    let expanded = quote! {
        use miniconf::Error;
        use miniconf::serde_json_core;

        impl StringSet for #name {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                let field = topic_parts.next().ok_or(Error::NameTooShort)?;
                let next = topic_parts.peek();

                if let Some(_next) = next {
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
