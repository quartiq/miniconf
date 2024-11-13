use darling::{uses_lifetimes, uses_type_params, util::Flag, FromField, FromMeta};
use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::spanned::Spanned;

#[derive(Debug, FromMeta, PartialEq, Clone, Default)]
pub struct Deny {
    serialize: Option<String>,
    deserialize: Option<String>,
    ref_any: Option<String>,
    mut_any: Option<String>,
}

#[derive(Debug, FromField, Clone)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,
    pub skip: Flag,
    pub typ: Option<syn::Type>,
    pub validate: Option<syn::Expr>,
    pub get: Option<syn::Expr>,
    pub get_mut: Option<syn::Expr>,
    pub rename: Option<syn::Ident>,
    pub defer: Option<syn::Expr>,
    #[darling(default)]
    pub deny: Deny,
}

uses_type_params!(TreeField, ty, typ);
uses_lifetimes!(TreeField, ty, typ);

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
        quote_spanned!(self.span()=> #i => <#typ as ::miniconf::TreeKey>::traverse_by_key(keys, func))
    }

    pub fn traverse_all(&self) -> TokenStream {
        let typ = self.typ();
        quote_spanned!(self.span()=> <#typ as ::miniconf::TreeKey>::traverse_all()?)
    }

    fn getter(&self, i: Option<usize>) -> TokenStream {
        if let Some(get) = &self.get {
            quote_spanned! { get.span()=>
                #get.map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            }
        } else if let Some(defer) = &self.defer {
            quote_spanned!(defer.span()=> ::core::result::Result::Ok(&#defer))
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
                #get_mut.map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            }
        } else if let Some(defer) = &self.defer {
            quote_spanned!(defer.span()=> ::core::result::Result::Ok(&mut #defer))
        } else if let Some(i) = i {
            let ident = self.ident_or_index(i);
            quote_spanned!(self.span()=> ::core::result::Result::Ok(&mut self.#ident))
        } else {
            quote_spanned!(self.span()=> ::core::result::Result::Ok(value))
        }
    }

    fn validator(&self) -> Option<TokenStream> {
        self.validate.as_ref().map(|validate| {
            quote_spanned! { validate.span()=>
                .and_then(|depth| #validate(depth)
                    .map_err(|msg| ::miniconf::Traversal::Invalid(0, msg).into())
                )
            }
        })
    }

    pub fn serialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        if let Some(s) = &self.deny.serialize {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::Traversal::Access(0, #s).into())
            }
        } else {
            let getter = self.getter(i);
            quote_spanned! { self.span()=>
                #getter
                    .and_then(|value|
                        ::miniconf::TreeSerialize::serialize_by_key(value, keys, ser)
                    )
            }
        }
    }

    pub fn deserialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        if let Some(s) = &self.deny.deserialize {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::Traversal::Access(0, #s).into())
            }
        } else {
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
    }

    pub fn ref_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        if let Some(s) = &self.deny.ref_any {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::Traversal::Access(0, #s).into())
            }
        } else {
            let getter = self.getter(i);
            quote_spanned! { self.span()=>
                #getter
                    .and_then(|item| ::miniconf::TreeAny::ref_any_by_key(item, keys))
            }
        }
    }

    pub fn mut_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        if let Some(s) = &self.deny.mut_any {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::Traversal::Access(0, #s).into())
            }
        } else {
            let getter_mut = self.getter_mut(i);
            quote_spanned! { self.span()=>
                #getter_mut
                    .and_then(|item| ::miniconf::TreeAny::mut_any_by_key(item, keys))
            }
        }
    }
}
