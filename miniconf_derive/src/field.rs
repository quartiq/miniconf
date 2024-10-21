use darling::{util::Flag, FromField};
use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::spanned::Spanned;

#[derive(Debug, FromField, Clone)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,
    pub skip: Flag,
    pub typ: Option<syn::Type>,
    pub validate: Option<syn::Path>,
    pub get: Option<syn::Path>,
    pub get_mut: Option<syn::Path>,
    pub rename: Option<syn::Ident>,
}

impl TreeField {
    fn span(&self) -> Span {
        self.ident
            .as_ref()
            .map(|i| i.span())
            .unwrap_or(self.ty.span())
    }

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
                quote_spanned!(self.span()=> #index)
            }
            Some(name) => quote_spanned!(self.span()=> #name),
        }
    }

    pub fn traverse_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `traverse_by_key()` args available.
        let typ = self.typ();
        quote_spanned! { self.span()=>
            #i => <#typ as ::miniconf::TreeKey>::traverse_by_key(keys, func)
        }
    }

    pub fn traverse_all(&self) -> TokenStream {
        let typ = self.typ();
        quote_spanned! { self.span()=>
            <#typ as ::miniconf::TreeKey>::traverse_all()?
        }
    }

    fn getter(&self, i: Option<usize>) -> TokenStream {
        if let Some(get) = &self.get {
            quote_spanned! { get.span()=>
                #get(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            }
        } else if let Some(i) = i {
            let ident = self.ident_or_index(i);
            quote_spanned!(self.span()=> ::core::result::Result::Ok(&self.#ident))
        } else {
            quote_spanned!(self.span()=> ::core::result::Result::Ok(value))
        }
    }

    fn getter_mut(&self, i: Option<usize>) -> TokenStream {
        if let Some(get_mut) = &self.get_mut {
            quote_spanned! { get_mut.span()=>
                #get_mut(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            }
        } else if let Some(i) = i {
            let ident = self.ident_or_index(i);
            quote_spanned!(self.span()=> ::core::result::Result::Ok(&mut self.#ident))
        } else {
            quote_spanned!(self.span()=> ::core::result::Result::Ok(value))
        }
    }

    fn validator(&self) -> TokenStream {
        if let Some(validate) = &self.validate {
            quote_spanned! { validate.span()=>
                .and_then(|value| #validate(self, value)
                    .map_err(|msg| ::miniconf::Traversal::Invalid(0, msg).into())
                )
            }
        } else {
            quote_spanned!(self.span()=> )
        }
    }

    pub fn serialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let getter = self.getter(i);
        quote_spanned! { self.span()=>
            #getter
                .and_then(|value|
                    ::miniconf::TreeSerialize::serialize_by_key(value, keys, ser))
        }
    }

    pub fn deserialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        let getter_mut = self.getter_mut(i);
        let validator = self.validator();
        quote_spanned! { self.span()=>
            #getter_mut
                .and_then(|item|
                    ::miniconf::TreeDeserialize::<'de>::deserialize_by_key(item, keys, de)
                )
                #validator
        }
    }

    pub fn ref_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let getter = self.getter(i);
        quote_spanned! { self.span()=>
            #getter
                .and_then(|item| ::miniconf::TreeAny::ref_any_by_key(item, keys))
        }
    }

    pub fn mut_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let getter_mut = self.getter_mut(i);
        quote_spanned! { self.span()=>
            #getter_mut
                .and_then(|item| ::miniconf::TreeAny::mut_any_by_key(item, keys))
        }
    }
}
