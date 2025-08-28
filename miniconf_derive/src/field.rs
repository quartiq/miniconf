use std::collections::BTreeMap;

use darling::{
    usage::{IdentSet, Purpose, UsesTypeParams},
    uses_lifetimes, uses_type_params,
    util::Flag,
    FromField, FromMeta, Result,
};
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned, ToTokens};
use syn::{parse_quote, parse_quote_spanned, spanned::Spanned};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TreeTrait {
    Key,
    Serialize,
    Deserialize,
    Any,
}

#[derive(Debug, FromMeta, PartialEq, Clone, Default)]
struct Deny {
    traverse: Option<String>,
    serialize: Option<String>,
    deserialize: Option<String>,
    probe: Option<String>,
    ref_any: Option<String>,
    mut_any: Option<String>,
}

#[derive(Debug, FromMeta, PartialEq, Clone, Default)]
struct With {
    traverse: Option<syn::Path>,
    traverse_all: Option<syn::Path>,
    serialize: Option<syn::Expr>,
    deserialize: Option<syn::Expr>,
    probe: Option<syn::Path>,
    ref_any: Option<syn::Expr>,
    mut_any: Option<syn::Expr>,
}

#[derive(Debug, FromField, Clone)]
#[darling(attributes(tree), forward_attrs(doc))]
pub(crate) struct TreeField {
    pub ident: Option<syn::Ident>,
    ty: syn::Type,
    pub skip: Flag,
    typ: Option<syn::Type>, // Type to defer to
    rename: Option<syn::Ident>,
    defer: Option<syn::Expr>, // Value to defer to
    #[darling(default)]
    with: With,
    #[darling(default)]
    deny: Deny,
    #[darling(default)]
    meta: BTreeMap<String, Option<String>>,
    #[darling(with=Self::parse_attrs)]
    attrs: Attrs,
}

#[derive(Debug, FromMeta, PartialEq, Clone, Default)]
struct Attrs {
    #[darling(multiple)]
    doc: Vec<String>,
}

uses_type_params!(TreeField, ty, typ);
uses_lifetimes!(TreeField, ty, typ);

impl TreeField {
    fn parse_attrs(attrs: Vec<syn::Attribute>) -> Result<Attrs> {
        // Attrs::from_list(attrs.try_into()?)
        Ok(Attrs::default())
    }

    pub fn span(&self) -> Span {
        self.ident
            .as_ref()
            .map(|i| i.span())
            .unwrap_or(self.ty.span())
    }

    pub fn typ(&self) -> &syn::Type {
        self.typ.as_ref().unwrap_or(&self.ty)
    }

    pub fn meta(&self) -> TokenStream {
        self.meta.iter().map(|(k, v)| quote!((#k, #v), )).collect()
    }

    pub fn bound(&self, trtr: TreeTrait, type_set: &IdentSet) -> Option<TokenStream> {
        if self
            .uses_type_params(&Purpose::BoundImpl.into(), type_set)
            .is_empty()
        {
            None
        } else {
            match trtr {
                TreeTrait::Key => Some(parse_quote!(::miniconf::TreeKey)),
                TreeTrait::Serialize => self
                    .deny
                    .serialize
                    .is_none()
                    .then_some(parse_quote!(::miniconf::TreeSerialize)),
                TreeTrait::Deserialize => (self.deny.deserialize.is_none()
                    || self.deny.probe.is_none())
                .then_some(parse_quote!(::miniconf::TreeDeserialize<'de>)),
                TreeTrait::Any => (self.deny.ref_any.is_none() || self.deny.mut_any.is_none())
                    .then_some(parse_quote!(::miniconf::TreeAny)),
            }
            .map(|bound: syn::TraitBound| {
                let ty = self.typ();
                quote_spanned!(self.span()=> #ty: #bound,)
            })
        }
    }

    pub fn name(&self) -> Option<&syn::Ident> {
        self.rename.as_ref().or(self.ident.as_ref())
    }

    fn value(&self, i: Option<usize>) -> syn::Expr {
        let def = if let Some(i) = i {
            // named or tuple struct field
            if let Some(name) = &self.ident {
                parse_quote_spanned!(self.span()=> self.#name)
            } else {
                let index = syn::Index::from(i);
                parse_quote_spanned!(self.span()=> self.#index)
            }
        } else {
            // enum variant newtype value
            parse_quote_spanned!(self.span()=> value)
        };
        self.defer.clone().unwrap_or(def)
    }

    pub fn serialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        if let Some(msg) = &self.deny.serialize {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::ValueError::Access(#msg).into())
            }
        } else {
            let value = self.value(i);
            let imp = self
                .with
                .serialize
                .as_ref()
                .map(|p| p.to_token_stream())
                .unwrap_or(quote!(#value.serialize_by_key));
            quote_spanned! { self.span()=> #imp(keys, ser) }
        }
    }

    pub fn deserialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        if let Some(msg) = &self.deny.deserialize {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::ValueError::Access(#msg).into())
            }
        } else {
            let value = self.value(i);
            let imp = self
                .with
                .deserialize
                .as_ref()
                .map(|p| p.to_token_stream())
                .unwrap_or(quote!(#value.deserialize_by_key));
            quote_spanned! { self.span()=> #imp(keys, de) }
        }
    }

    pub fn probe_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `probe_by_key()` args available.
        if let Some(msg) = &self.deny.probe {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::ValueError::Access(#msg).into())
            }
        } else {
            let typ = self.typ();
            let imp = self
                .with
                .probe
                .as_ref()
                .map(|i| i.to_token_stream())
                .unwrap_or(quote!(<#typ as ::miniconf::TreeDeserialize::<'de>>::probe_by_key));
            quote_spanned!(self.span()=> #i => #imp(keys, de))
        }
    }

    pub fn ref_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        if let Some(msg) = &self.deny.ref_any {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::ValueError::Access(#msg))
            }
        } else {
            let value = self.value(i);
            let imp = self
                .with
                .ref_any
                .as_ref()
                .map(|p| p.to_token_stream())
                .unwrap_or(quote!(#value.ref_any_by_key));
            quote_spanned! { self.span()=> #imp(keys) }
        }
    }

    pub fn mut_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        if let Some(msg) = &self.deny.mut_any {
            quote_spanned! { self.span()=> ::core::result::Result::Err(
                ::miniconf::ValueError::Access(#msg))
            }
        } else {
            let value = self.value(i);
            let imp = self
                .with
                .mut_any
                .as_ref()
                .map(|p| p.to_token_stream())
                .unwrap_or(quote!(#value.mut_any_by_key));
            quote_spanned! { self.span()=> #imp(keys) }
        }
    }
}
