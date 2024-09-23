use darling::{util::Flag, FromField};
use proc_macro2::TokenStream;
use quote::quote;

#[derive(Debug, FromField, Clone)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,
    #[darling(default)]
    pub depth: usize,
    pub skip: Flag,
    pub typ: Option<syn::Type>,
    pub validate: Option<syn::Path>,
    pub get: Option<syn::Path>,
    pub get_mut: Option<syn::Path>,
    pub rename: Option<syn::Ident>,
}

impl TreeField {
    pub fn typ(&self) -> &syn::Type {
        self.typ.as_ref().unwrap_or(&self.ty)
    }

    pub fn name(&self) -> Option<&syn::Ident> {
        self.rename.as_ref().or(self.ident.as_ref())
    }

    fn ident_or_index(&self, i: usize) -> TokenStream {
        match &self.ident {
            None => {
                let index = syn::Index::from(i);
                quote! { #index }
            }
            Some(name) => quote! { #name },
        }
    }

    pub fn traverse_by_key(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `traverse_by_key()` args available.
        let depth = self.depth;
        if depth > 0 {
            let typ = self.typ();
            Some(quote! {
                #i => <#typ as ::miniconf::TreeKey<#depth>>::traverse_by_key(keys, func)
            })
        } else {
            None
        }
    }

    pub fn metadata(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `metadata()` args available.
        let depth = self.depth;
        if depth > 0 {
            let typ = self.typ();
            Some(quote! {
                #i => <#typ as ::miniconf::TreeKey<#depth>>::metadata()
            })
        } else {
            None
        }
    }

    fn getter(&self, i: usize, value: bool) -> TokenStream {
        let ident = self.ident_or_index(i);
        match (&self.get, value) {
            (Some(get), _) => quote! {
                #get(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            },
            (None, false) => quote! { Ok(&self.#ident) },
            (None, true) => quote! { Ok(value) },
        }
    }

    fn getter_mut(&self, i: usize, value: bool) -> TokenStream {
        let ident = self.ident_or_index(i);
        match (&self.get_mut, value) {
            (Some(get_mut), _) => quote!(
                #get_mut(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            ),
            (None, false) => quote! { Ok(&mut self.#ident) },
            (None, true) => quote! { Ok(value) },
        }
    }

    fn validator(&self) -> TokenStream {
        match &self.validate {
            Some(validate) => quote! {
                .and_then(|value| #validate(self, value)
                    .map_err(|msg| ::miniconf::Traversal::Invalid(0, msg).into())
                )
            },
            None => quote! {},
        }
    }

    fn lhs(&self, i: usize, ident: Option<&syn::Ident>) -> TokenStream {
        if let Some(ident) = ident {
            quote! { (Self::#ident(value), #i) }
        } else {
            quote! { #i }
        }
    }

    pub fn serialize_by_key(&self, i: usize, ident: Option<&syn::Ident>) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let lhs = self.lhs(i, ident);
        let depth = self.depth;
        let getter = self.getter(i, ident.is_some());
        if depth > 0 {
            quote! {
                #lhs => #getter
                    .and_then(|value|
                        ::miniconf::TreeSerialize::<#depth>::serialize_by_key(value, keys, ser))
            }
        } else {
            quote! {
                #lhs => #getter
                    .and_then(|value|
                        ::miniconf::Serialize::serialize(value, ser)
                        .map_err(|err| ::miniconf::Error::Inner(0, err))
                        .and(Ok(0))
                    )
            }
        }
    }

    pub fn deserialize_by_key(&self, i: usize, ident: Option<&syn::Ident>) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        let lhs = self.lhs(i, ident);
        let depth = self.depth;
        let getter_mut = self.getter_mut(i, ident.is_some());
        let validator = self.validator();
        if depth > 0 {
            quote! {
                #lhs => #getter_mut
                    .and_then(|item|
                        ::miniconf::TreeDeserialize::<'de, #depth>::deserialize_by_key(item, keys, de)
                    )
                    #validator
            }
        } else {
            quote! {
                #lhs => ::miniconf::Deserialize::deserialize(de)
                    .map_err(|err| ::miniconf::Error::Inner(0, err))
                    #validator
                    .and_then(|new|
                        #getter_mut.and_then(|item| {
                            *item = new;
                            Ok(0)
                        })
                    )
            }
        }
    }

    pub fn ref_any_by_key(&self, i: usize, ident: Option<&syn::Ident>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let lhs = self.lhs(i, ident);
        let depth = self.depth;
        let getter = self.getter(i, ident.is_some());
        if depth > 0 {
            quote! {
                #lhs => #getter
                    .and_then(|item| ::miniconf::TreeAny::<#depth>::ref_any_by_key(item, keys))
            }
        } else {
            quote! {
                #lhs => #getter.map(|item| item as &dyn ::core::any::Any)
            }
        }
    }

    pub fn mut_any_by_key(&self, i: usize, ident: Option<&syn::Ident>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let lhs = self.lhs(i, ident);
        let depth = self.depth;
        let getter_mut = self.getter_mut(i, ident.is_some());
        if depth > 0 {
            quote! {
                #lhs => #getter_mut
                    .and_then(|item| ::miniconf::TreeAny::<#depth>::mut_any_by_key(item, keys))
            }
        } else {
            quote! {
                #lhs => #getter_mut.map(|item| item as &mut dyn ::core::any::Any)
            }
        }
    }
}
