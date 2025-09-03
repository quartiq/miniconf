use std::collections::BTreeMap;

use darling::{
    ast::{self, Data, Style},
    usage::{GenericsExt, LifetimeRefSet, Purpose, UsesLifetimes},
    util::Flag,
    Error, FromDeriveInput, FromVariant, Result,
};
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_quote, WhereClause};

use crate::field::{doc_to_meta, TreeField, TreeTrait};

#[derive(Debug, FromVariant, Clone)]
#[darling(
    attributes(tree),
    forward_attrs(doc),
    supports(newtype, tuple, unit),
    and_then=Self::parse)]
pub struct TreeVariant {
    ident: syn::Ident,
    rename: Option<syn::Ident>,
    skip: Flag,
    fields: ast::Fields<TreeField>,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    meta: BTreeMap<String, Option<String>>,
}

impl TreeVariant {
    fn parse(mut self) -> Result<Self> {
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
        assert_eq!(self.fields.len(), 1); // Only newtypes currently
        self.fields.fields.first().unwrap()
    }

    fn name(&self) -> &syn::Ident {
        self.rename.as_ref().unwrap_or(&self.ident)
    }

    pub fn meta(&self) -> TokenStream {
        self.meta.iter().map(|(k, v)| quote!((#k, #v), )).collect()
    }
}

#[derive(Debug, FromDeriveInput, Clone)]
#[darling(
    attributes(tree),
    forward_attrs(doc),
    supports(struct_named, struct_newtype, struct_tuple, enum_newtype, enum_tuple, enum_unit),
    and_then=Self::parse)]
pub struct Tree {
    ident: syn::Ident,
    generics: syn::Generics,
    flatten: Flag,
    data: Data<TreeVariant, TreeField>,
    doc: Flag,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    meta: BTreeMap<String, String>,
}

impl Tree {
    fn parse(mut self) -> Result<Self> {
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
                        // TODO: If non-terminal tuple fields are skipped, there is a gap in the indices.
                        // This could be lifted by correct indices.
                        Error::custom("Can only `skip` terminal tuple struct fields")
                            .with_span(&f.skip.span()),
                    );
                }
            }
            Data::Enum(variants) => {
                variants.retain(|v| !v.skip.is_present() && !v.fields.is_empty());
                for v in variants.iter() {
                    if v.fields.len() != 1 {
                        return Err(Error::custom(
                            "Only newtype (single field tuple) and unit enum variants are supported.",
                        )
                        .with_span(&v.ident.span()));
                    }
                    if !v.field().meta().is_empty() {
                        return Err(Error::custom(
                            "Outer metadata must be placed on the variant, not on the tuple field.",
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
        self.doc_to_meta()?;
        Ok(self)
    }

    fn doc_to_meta(&mut self) -> Result<()> {
        if self.doc.is_present() {
            doc_to_meta(&self.attrs, &mut self.meta)?;
            match &mut self.data {
                Data::Struct(fields) => {
                    for field in fields.fields.iter_mut() {
                        doc_to_meta(&field.attrs, &mut field.meta)?;
                    }
                }
                Data::Enum(variants) => {
                    for variant in variants.iter_mut() {
                        let field = variant.fields.fields.first_mut().unwrap();
                        doc_to_meta(&variant.attrs, &mut field.meta)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn fields(&self) -> Vec<&TreeField> {
        match &self.data {
            Data::Struct(fields) => fields.iter().collect(),
            Data::Enum(variants) => variants.iter().map(|v| v.field()).collect(),
        }
    }

    fn arms(
        &self,
        mut func: impl FnMut(&TreeField, Option<usize>) -> TokenStream,
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
            quote!(::core::result::Result::<usize, ::miniconf::ValueError>::Ok(
                0
            ))
        } else {
            quote!(<Self as ::miniconf::TreeSchema>::SCHEMA.next(&mut keys))
        }
    }

    fn meta(&self) -> TokenStream {
        self.meta.iter().map(|(k, v)| quote!((#k, #v), )).collect()
    }

    pub fn tree_schema(&self) -> TokenStream {
        let ident = &self.ident;
        let (impl_generics, ty_generics, orig_where_clause) = self.generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Key, orig_where_clause);
        let schema = if self.flatten.is_present() {
            let typ = self.fields().first().unwrap().typ();
            quote! { <#typ as ::miniconf::TreeSchema>::SCHEMA }
        } else {
            let internal = match &self.data {
                Data::Struct(fields) => {
                    match fields.style {
                        Style::Tuple => {
                            let numbered: TokenStream = fields
                                .iter()
                                .map(|f| {
                                    let typ = f.typ();
                                    let meta = f.meta();
                                    quote_spanned! { f.span()=> ::miniconf::Numbered {
                                        schema: <#typ as ::miniconf::TreeSchema>::SCHEMA,
                                        meta: Some(&[#meta]),
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
                                    let meta = f.meta();
                                    quote_spanned! { name.span()=> ::miniconf::Named {
                                        name: stringify!(#name),
                                        schema: <#typ as ::miniconf::TreeSchema>::SCHEMA,
                                        meta: Some(&[#meta]),
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
                            let meta = v.meta();
                            quote_spanned! { v.field().span()=> ::miniconf::Named {
                                name: stringify!(#name),
                                schema: <#typ as ::miniconf::TreeSchema>::SCHEMA,
                                meta: Some(&[#meta]),
                            }, }
                        })
                        .collect();
                    quote! { ::miniconf::Internal::Named(&[#named]) }
                }
            };
            let meta = self.meta();
            quote! { &::miniconf::Schema {
                meta: Some(&[#meta]),
                internal: Some(#internal),
            } }
        };
        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeSchema for #ident #ty_generics #where_clause {
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
                fn serialize_by_key<S: ::miniconf::Serializer>(
                    &self,
                    mut keys: impl ::miniconf::Keys,
                    ser: S
                ) -> ::core::result::Result<S::Ok, ::miniconf::SerDeError<S::Error>>
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
                fn deserialize_by_key<D: ::miniconf::Deserializer<'de>>(
                    &mut self,
                    mut keys: impl ::miniconf::Keys,
                    de: D
                ) -> ::core::result::Result<(), ::miniconf::SerDeError<D::Error>>
                {
                    let index = #index?;
                    match #mat {
                        #(#deserialize_arms ,)*
                        _ => #default
                    }
                }

            fn probe_by_key<D: ::miniconf::Deserializer<'de>>(
                mut keys: impl ::miniconf::Keys,
                de: D
            ) -> ::core::result::Result<(), ::miniconf::SerDeError<D::Error>>
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
                fn ref_any_by_key(
                    &self,
                    mut keys: impl ::miniconf::Keys
                ) -> ::core::result::Result<&dyn ::core::any::Any, ::miniconf::ValueError>
                {
                    let index = #index?;
                    match #mat {
                        #(#ref_arms ,)*
                        _ => #default
                    }
                }

                fn mut_any_by_key(
                    &mut self,
                    mut keys: impl ::miniconf::Keys
                ) -> ::core::result::Result<&mut dyn ::core::any::Any, ::miniconf::ValueError>
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
