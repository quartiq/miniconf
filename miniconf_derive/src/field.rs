use darling::{util::Flag, FromField};
use proc_macro2::TokenStream;
use quote::quote;

#[derive(Debug, FromField, Clone)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    // pub vis: syn::Visibility,
    pub ty: syn::Type,
    // attrs: Vec<syn::Attribute>,
    #[darling(skip)]
    pub variant: bool,
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
    pub(crate) fn typ(&self) -> &syn::Type {
        self.typ.as_ref().unwrap_or(&self.ty)
    }

    pub(crate) fn name(&self) -> Option<&syn::Ident> {
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

    pub(crate) fn traverse_by_key(&self, i: usize) -> Option<TokenStream> {
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

    pub(crate) fn metadata(&self, i: usize) -> Option<TokenStream> {
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

    fn getter(&self, i: usize) -> TokenStream {
        let ident = self.ident_or_index(i);
        match &self.get {
            Some(get) => quote! {
                #get(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            },
            None => {
                if self.variant {
                    quote! { Ok(#ident) }
                } else {
                    quote! { Ok(&self.#ident) }
                }
            }
        }
    }

    fn getter_mut(&self, i: usize) -> TokenStream {
        let ident = self.ident_or_index(i);
        match &self.get_mut {
            Some(get_mut) => quote!(
                #get_mut(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            ),
            None => {
                if self.variant {
                    quote! { Ok(#ident) }
                } else {
                    quote! { Ok(&mut self.#ident) }
                }
            }
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

    pub(crate) fn serialize_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let depth = self.depth;
        let getter = self.getter(i);
        if depth > 0 {
            quote! {
                #i => #getter
                    .and_then(|value|
                        ::miniconf::TreeSerialize::<#depth>::serialize_by_key(value, keys, ser))
            }
        } else {
            quote! {
                #i => #getter
                    .and_then(|value|
                        ::miniconf::Serialize::serialize(value, ser)
                        .map_err(|err| ::miniconf::Error::Inner(0, err))
                        .and(Ok(0))
                    )
            }
        }
    }

    pub(crate) fn deserialize_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        let depth = self.depth;
        let getter_mut = self.getter_mut(i);
        let validator = self.validator();
        if depth > 0 {
            quote! {
                #i => #getter_mut
                    .and_then(|item|
                        ::miniconf::TreeDeserialize::<'de, #depth>::deserialize_by_key(item, keys, de)
                    )
                    #validator
            }
        } else {
            quote! {
                #i => ::miniconf::Deserialize::deserialize(de)
                    .map_err(|err| ::miniconf::Error::Inner(0, err))
                    #validator
                    .and_then(|value|
                        #getter_mut.and_then(|item| {
                            *item = value;
                            Ok(0)
                        })
                    )
            }
        }
    }

    pub(crate) fn ref_any_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.depth;
        let getter = self.getter(i);
        if depth > 0 {
            quote! {
                #i => #getter
                    .and_then(|value| ::miniconf::TreeAny::<#depth>::ref_any_by_key(value, keys))
            }
        } else {
            quote! {
                #i => #getter.map(|value| value as &dyn ::core::any::Any)
            }
        }
    }

    pub(crate) fn mut_any_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.depth;
        let getter_mut = self.getter_mut(i);
        if depth > 0 {
            quote! {
                #i => #getter_mut
                    .and_then(|value| ::miniconf::TreeAny::<#depth>::mut_any_by_key(value, keys))
            }
        } else {
            quote! {
                #i => #getter_mut.map(|value| value as &mut dyn ::core::any::Any)
            }
        }
    }
}
