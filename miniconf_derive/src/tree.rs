use darling::{
    ast::{self, Data, Style},
    usage::{GenericsExt, LifetimeRefSet, Purpose, UsesLifetimes},
    util::Flag,
    Error, FromDeriveInput, FromVariant,
};
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_quote, token::Token, WhereClause};

use crate::field::{TreeField, TreeTrait};

#[derive(Debug, FromVariant, Clone)]
#[darling(attributes(tree), supports(newtype, tuple, unit), and_then=Self::parse)]
pub struct TreeVariant {
    ident: syn::Ident,
    rename: Option<syn::Ident>,
    skip: Flag,
    fields: ast::Fields<TreeField>,
}

impl TreeVariant {
    fn parse(mut self) -> darling::Result<Self> {
        assert!(!self.fields.is_struct());
        while self
            .fields
            .fields
            .last()
            .map(|f| f.skip.is_present())
            .unwrap_or_default()
        {
            self.fields.fields.pop();
        }
        if let Some(f) = self.fields.iter().find(|f| f.skip.is_present()) {
            return Err(
                Error::custom("Can only `skip` terminal tuple variant fields")
                    .with_span(&f.skip.span()),
            );
        }
        Ok(self)
    }

    fn field(&self) -> &TreeField {
        // assert!(self.fields.is_newtype()); // Don't do this since we modified it with skip
        assert!(self.fields.len() == 1); // Only newtypes currently
        self.fields.fields.first().unwrap()
    }

    fn name(&self) -> &syn::Ident {
        self.rename.as_ref().unwrap_or(&self.ident)
    }
}

#[derive(Debug, FromDeriveInput, Clone)]
#[darling(attributes(tree), supports(struct_named, struct_newtype, struct_tuple, enum_newtype, enum_tuple, enum_unit), and_then=Self::parse)]
#[darling()]
pub struct Tree {
    ident: syn::Ident,
    generics: syn::Generics,
    flatten: Flag,
    data: Data<TreeVariant, TreeField>,
}

impl Tree {
    fn parse(mut self) -> darling::Result<Self> {
        match &mut self.data {
            Data::Struct(fields) => {
                while fields
                    .fields
                    .last()
                    .map(|f| f.ident.is_none() && f.skip.is_present())
                    .unwrap_or_default()
                {
                    fields.fields.pop();
                }
                fields
                    .fields
                    .retain(|f| f.ident.is_none() || !f.skip.is_present());
                if let Some(f) = fields.fields.iter().find(|f| f.skip.is_present()) {
                    return Err(
                        // Note(design) If non-terminal fields are skipped, there is a gap in the indices.
                        // This could be lifted with an index map.
                        Error::custom("Can only `skip` terminal tuple struct fields")
                            .with_span(&f.skip.span()),
                    );
                }
            }
            Data::Enum(variants) => {
                variants.retain(|v| !(v.skip.is_present() || v.fields.is_empty()));
                for v in variants.iter() {
                    if v.fields.len() != 1 {
                        return Err(Error::custom(
                            "Only newtype (single field tuple) and unit enum variants are supported.",
                        )
                        .with_span(&v.ident.span()));
                    }
                }
            }
        }
        if self.flatten.is_present() && self.fields().len() != 1 {
            return Err(Error::custom("Can't flatten multiple fields/variants")
                .with_span(&self.flatten.span()));
        }
        if self.fields().is_empty() {
            return Err(Error::custom("Internal nodes must have at least one leaf")
                .with_span(&self.ident.span()));
        }
        Ok(self)
    }

    fn fields(&self) -> Vec<&TreeField> {
        match &self.data {
            Data::Struct(fields) => fields.iter().collect(),
            Data::Enum(variants) => variants.iter().map(|v| v.field()).collect(),
        }
    }

    fn arms<F: FnMut(&TreeField, Option<usize>) -> TokenStream>(
        &self,
        mut func: F,
    ) -> (TokenStream, Vec<TokenStream>, TokenStream) {
        match &self.data {
            Data::Struct(fields) => (
                quote!(index),
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let rhs = func(f, Some(i));
                        quote!(#i => #rhs)
                    })
                    .collect(),
                quote!(::core::unreachable!()),
            ),
            Data::Enum(variants) => (
                quote!((self, index)),
                variants
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let ident = &v.ident;
                        let rhs = func(v.field(), None);
                        quote!((Self::#ident(value, ..), #i) => #rhs)
                    })
                    .collect(),
                quote!(::core::result::Result::Err(
                    ::miniconf::ValueError::Absent.into()
                )),
            ),
        }
    }

    fn bound_generics(
        &self,
        traite: TreeTrait,
        where_clause: Option<&WhereClause>,
    ) -> Option<syn::WhereClause> {
        let type_set = self.generics.declared_type_params();
        let bounds: TokenStream = self
            .fields()
            .iter()
            .filter_map(|f| f.bound(traite, &type_set))
            .collect();
        if bounds.is_empty() {
            where_clause.cloned()
        } else if where_clause.is_some() {
            Some(parse_quote! { #where_clause #bounds })
        } else {
            Some(parse_quote! { where #bounds })
        }
    }

    fn index(&self) -> TokenStream {
        if self.flatten.is_present() {
            quote!(::core::result::Result::<(), ::miniconf::ValueError>::Ok())
        } else {
            quote!(<Self as ::miniconf::TreeKey>::SCHEMA.next(&mut keys))
        }
    }

    pub fn tree_key(&self) -> TokenStream {
        let ident = &self.ident;
        let (impl_generics, ty_generics, orig_where_clause) = self.generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Key, orig_where_clause);
        let internal = match &self.data {
            Data::Struct(fields) => {
                match fields.style {
                    Style::Tuple => {
                        let numbered: TokenStream = fields
                            .iter()
                            .map(|f| {
                                // ident is Some
                                let typ = f.typ();

                                quote_spanned! { f.span()=> ::miniconf::Numbered {
                                    schema: <#typ as ::miniconf::TreeKey>::SCHEMA,
                                    meta: None,
                                }, }
                            })
                            .collect();
                        quote! { ::miniconf::Internal::Numbered(&[#numbered]) }
                    }
                    Style::Struct => {
                        let named: TokenStream = fields
                            .iter()
                            .map(|f| {
                                // ident is Some
                                let name = f.name().unwrap();
                                let typ = f.typ();

                                quote_spanned! { name.span()=> ::miniconf::Named {
                                    name: stringify!(#name),
                                    schema: <#typ as ::miniconf::TreeKey>::SCHEMA,
                                    meta: None,
                                }, }
                            })
                            .collect();
                        quote! { ::miniconf::Internal::Named(&[#named]) }
                    }
                    Style::Unit => unreachable!(),
                }
            }
            Data::Enum(variants) => {
                let named: TokenStream = variants
                    .iter()
                    .map(|v| {
                        let name = v.name();
                        // ident is Some
                        let typ = v.field().typ();
                        quote_spanned! { v.field().span()=> ::miniconf::Named {
                            name: stringify!(#name),
                            schema: <#typ as ::miniconf::TreeKey>::SCHEMA,
                            meta: None,
                        }, }
                    })
                    .collect();
                quote! { ::miniconf::Internal::Named(&[#named]) }
            }
        };
        let schema = if self.flatten.is_present() {
            let typ = self.fields().first().unwrap().typ();
            quote! { <#typ as ::miniconf::TreeKey>::SCHEMA }
        } else {
            quote! { &::miniconf::Schema {
                meta: None, // TODO
                internal: Some(#internal),
            } }
        };
        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeKey for #ident #ty_generics #where_clause {
                const SCHEMA: &'static ::miniconf::Schema = #schema;
            }
        }
    }

    pub fn tree_serialize(&self) -> TokenStream {
        let ident = &self.ident;
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Serialize, where_clause);
        let index = self.index();
        let (mat, arms, default) = self.arms(|f, i| f.serialize_by_key(i));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeSerialize for #ident #ty_generics #where_clause {
                fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> ::core::result::Result<S::Ok, ::miniconf::SerDeError<S::Error>>
                where
                    K: ::miniconf::Keys,
                    S: ::miniconf::Serializer,
                {
                    let index = #index?;
                    match #mat {
                        #(#arms ,)*
                        _ => #default
                    }
                }
            }
        }
    }

    pub fn tree_deserialize(&self) -> TokenStream {
        let ty_generics = self.generics.split_for_impl().1;
        let lifetimes = self.generics.declared_lifetimes();
        let mut de: syn::LifetimeParam = parse_quote!('de);
        de.bounds.extend(
            self.fields()
                .iter()
                .flat_map(|f| f.uses_lifetimes(&Purpose::BoundImpl.into(), &lifetimes))
                .collect::<LifetimeRefSet>()
                .into_iter()
                .cloned(),
        );
        let mut generics = self.generics.clone();
        generics.params.push(syn::GenericParam::Lifetime(de));
        let (impl_generics, _, where_clause) = generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Deserialize, where_clause);
        let index = self.index();
        let ident = &self.ident;
        let (mat, deserialize_arms, default) = self.arms(|f, i| f.deserialize_by_key(i));
        let fields = self.fields();
        let probe_arms = fields.iter().enumerate().map(|(i, f)| f.probe_by_key(i));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeDeserialize<'de> for #ident #ty_generics #where_clause {
                fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> ::core::result::Result<(), ::miniconf::SerDeError<D::Error>>
                where
                    K: ::miniconf::Keys,
                    D: ::miniconf::Deserializer<'de>,
                {
                    let index = #index?;
                    match #mat {
                        #(#deserialize_arms ,)*
                        _ => #default
                    }
                }

            fn probe_by_key<K, D>(mut keys: K, de: D) -> ::core::result::Result<(), ::miniconf::SerDeError<D::Error>>
                where
                    K: ::miniconf::Keys,
                    D: ::miniconf::Deserializer<'de>,
                {
                    let index = #index?;
                    match index {
                        #(#probe_arms ,)*
                        _ => unreachable!()
                    }
                }
            }
        }
    }

    pub fn tree_any(&self) -> TokenStream {
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Any, where_clause);
        let index = self.index();
        let ident = &self.ident;
        let (mat, ref_arms, default) = self.arms(|f, i| f.ref_any_by_key(i));
        let (_, mut_arms, _) = self.arms(|f, i| f.mut_any_by_key(i));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeAny for #ident #ty_generics #where_clause {
                fn ref_any_by_key<K>(&self, mut keys: K) -> ::core::result::Result<&dyn ::core::any::Any, ::miniconf::ValueError>
                where
                    K: ::miniconf::Keys,
                {
                    let index = #index?;
                    match #mat {
                        #(#ref_arms ,)*
                        _ => #default
                    }
                }

                fn mut_any_by_key<K>(&mut self, mut keys: K) -> ::core::result::Result<&mut dyn ::core::any::Any, ::miniconf::ValueError>
                where
                    K: ::miniconf::Keys,
                {
                    let index = #index?;
                    match #mat {
                        #(#mut_arms ,)*
                        _ => #default
                    }
                }
            }
        }
    }
}
