use proc_macro::{TokenStream};
use syn::{parse_macro_input, DeriveInput};
use quote::quote;

#[proc_macro_derive(StringSet)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = input.ident;
    let fields = if let syn::Data::Struct(syn::DataStruct {
        fields: syn::Fields::Named(syn::FieldsNamed { ref named, .. }),
        ..
    }) = input.data {
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
                self.#name = serde_json_core::from_str(value)
                    .map_err(|_|{()})?.0;
                Ok(())
            }
        }
    });

    let expanded = quote! {
        impl StringSet for #name {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &str) ->
            Result<(),()> {
                let field = topic_parts.next().ok_or(())?;
                let next = topic_parts.peek();

                if let Some(_next) = next {
                    match field {
                        #(#recurse_match_arms ,)*
                        _ => Err(())
                    }

                } else {
                    match field {
                        #(#direct_set_match_arms ,)*
                        _ => Err(())
                    }
                }
            }

        }
        
    };

    TokenStream::from(expanded)
}
