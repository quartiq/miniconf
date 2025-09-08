use std::collections::BTreeMap;

use darling::{
    usage::{IdentSet, Purpose, UsesTypeParams},
    uses_lifetimes, uses_type_params,
    util::Flag,
    FromField,
};
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{parse_quote, parse_quote_spanned, spanned::Spanned};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TreeTrait {
    Key,
    Serialize,
    Deserialize,
    Any,
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
    with: Option<syn::Path>,
    #[darling(default)]
    pub meta: BTreeMap<String, String>,
    pub attrs: Vec<syn::Attribute>,
}

uses_type_params!(TreeField, ty, typ);
uses_lifetimes!(TreeField, ty, typ);

impl TreeField {
    pub fn span(&self) -> Span {
        self.ident
            .as_ref()
            .map(|i| i.span())
            .unwrap_or(self.ty.span())
    }

    fn typ(&self) -> &syn::Type {
        self.typ.as_ref().unwrap_or(&self.ty)
    }

    pub fn schema(&self) -> TokenStream {
        if let Some(all) = self.with.as_ref() {
            quote_spanned!(self.span()=> #all::SCHEMA)
        } else {
            let typ = self.typ();
            quote_spanned!(self.span()=> <#typ as ::miniconf::TreeSchema>::SCHEMA)
        }
    }

    pub fn bound(&self, trtr: TreeTrait, type_set: &IdentSet) -> Option<TokenStream> {
        if self
            .uses_type_params(&Purpose::BoundImpl.into(), type_set)
            .is_empty()
            || self.with.is_some()
        {
            None
        } else {
            let bound: syn::TraitBound = match trtr {
                TreeTrait::Key => parse_quote!(::miniconf::TreeSchema),
                TreeTrait::Serialize => parse_quote!(::miniconf::TreeSerialize),
                TreeTrait::Deserialize => parse_quote!(::miniconf::TreeDeserialize<'de>),
                TreeTrait::Any => parse_quote!(::miniconf::TreeAny),
            };
            let ty = self.typ();
            Some(quote_spanned!(self.span()=> #ty: #bound,))
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
        let value = self.value(i);
        let imp = self
            .with
            .as_ref()
            .map(|m| quote!(#m::serialize_by_key(&#value, keys, ser)))
            .unwrap_or(quote!(#value.serialize_by_key(keys, ser)));
        quote_spanned! { self.span()=> #imp }
    }

    pub fn deserialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        let value = self.value(i);
        let imp = self
            .with
            .as_ref()
            .map(|m| quote!(#m::deserialize_by_key(&mut #value, keys, de)))
            .unwrap_or(quote!(#value.deserialize_by_key(keys, de)));
        quote_spanned! { self.span()=> #imp }
    }

    pub fn probe_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `probe_by_key()` args available.
        let typ = self.typ();
        let imp = self
            .with
            .as_ref()
            .map(|m| quote!(#m::probe_by_key::<'de, #typ, _>(keys, de)))
            .unwrap_or(
                quote!(<#typ as ::miniconf::TreeDeserialize::<'de>>::probe_by_key(keys, de)),
            );
        quote_spanned! { self.span()=> #i => #imp }
    }

    pub fn ref_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let value = self.value(i);
        let imp = self
            .with
            .as_ref()
            .map(|m| quote!(#m::ref_any_by_key(&#value, keys)))
            .unwrap_or(quote!(#value.ref_any_by_key(keys)));
        quote_spanned! { self.span()=> #imp }
    }

    pub fn mut_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let value = self.value(i);
        let imp = self
            .with
            .as_ref()
            .map(|m| quote!(#m::mut_any_by_key(&mut #value, keys)))
            .unwrap_or(quote!(#value.mut_any_by_key(keys)));
        quote_spanned! { self.span()=> #imp }
    }
}
