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

    //let recurse_match_arms = fields.iter().filter(|f| {
    //    use syn::Type;
    //    match f {
    //        Array => true,

    //        _ => false
    //    }

    //})
    

    let normal_match_arms = fields.iter().map(|f| {
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
        impl #name {
            pub fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &str) ->
            Result<(),()> {
                let field = topic_parts.next().ok_or(())?;
                let next = topic_parts.peek();

                if let Some(_next) = next {
                    match field {
                        _ => Err(())
                    }

                } else {
                    match field {
                        #(#normal_match_arms,)*
                        _ => Err(())
                    }
                }
            }

        }
        
    };

    TokenStream::from(expanded)
}
