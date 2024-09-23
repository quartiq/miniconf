use darling::{
    ast,
    util::Flag,
    FromVariant,
};
use proc_macro2::TokenStream;
use quote::quote;

use crate::field::TreeField;

#[derive(Debug, FromVariant, Clone)]
#[darling(attributes(tree))]
pub struct TreeVariant {
    pub ident: syn::Ident,
    pub rename: Option<syn::Ident>,
    // pub flatten: Flag, // FIXME: implement
    pub skip: Flag,
    pub fields: ast::Fields<TreeField>,
}

impl TreeVariant {
    pub(crate) fn field(&self) -> &TreeField {
        assert!(self.fields.len() == 1);
        self.fields.fields.first().unwrap()
    }

    pub(crate) fn name(&self) -> &syn::Ident {
        self.rename.as_ref().unwrap_or(&self.ident)
    }

    pub(crate) fn traverse_by_key(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `traverse_by_key()` args available.
        let depth = self.field().depth;
        if depth > 0 {
            let typ = self.field().typ();
            Some(quote! {
                #i => <#typ as ::miniconf::TreeKey<#depth>>::traverse_by_key(keys, func)
            })
        } else {
            None
        }
    }

    pub(crate) fn metadata(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `metadata()` args available.
        let depth = self.field().depth;
        if depth > 0 {
            let typ = self.field().typ();
            Some(quote! {
                #i => <#typ as ::miniconf::TreeKey<#depth>>::metadata()
            })
        } else {
            None
        }
    }

    pub(crate) fn serialize_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let depth = self.field().depth;
        let ident = &self.ident;
        if depth > 0 {
            quote! {
                (Self::#ident(value), #i) =>
                    ::miniconf::TreeSerialize::<#depth>::serialize_by_key(value, keys, ser)
            }
        } else {
            quote! {
                (Self::#ident(value), #i) =>
                    ::miniconf::Serialize::serialize(value, ser)
                    .map_err(|err| ::miniconf::Error::Inner(0, err))
                    .and(Ok(0))
            }
        }
    }

    pub(crate) fn deserialize_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let depth = self.field().depth;
        let ident = &self.ident;
        if depth > 0 {
            quote! {
                (Self::#ident(value), #i) =>
                    ::miniconf::TreeDeserialize::<'de, #depth>::deserialize_by_key(value, keys, de)
            }
        } else {
            quote! {
                (Self::#ident(value), #i) =>
                    ::miniconf::Deserialize::deserialize(de)
                    .map_err(|err| ::miniconf::Error::Inner(0, err))
                    .and_then(|new| {
                        *value = new;
                        Ok(0)
                    })
            }
        }
    }

    pub(crate) fn ref_any_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.field().depth;
        let ident = &self.ident;
        if depth > 0 {
            quote! {
                (Self::#ident(value), #i) => ::miniconf::TreeAny::<#depth>::ref_any_by_key(value, keys)
            }
        } else {
            quote! {
                (Self::#ident(value), #i) => value as &dyn ::core::any::Any
            }
        }
    }

    pub(crate) fn mut_any_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.field().depth;
        let ident = &self.ident;
        if depth > 0 {
            quote! {
                (Self::#ident(value), #i) => ::miniconf::TreeAny::<#depth>::mut_any_by_key(value, keys)
            }
        } else {
            quote! {
                (Self::#ident(value), #i) => value as &mut dyn ::core::any::Any
            }
        }
    }
}
