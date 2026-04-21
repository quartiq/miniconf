use darling::{
    Error, FromDeriveInput, FromVariant, Result,
    ast::{self, Data, Style},
    usage::{GenericsExt, LifetimeRefSet, Purpose, UsesLifetimes},
    util::{Flag, Override, SpannedValue},
};
use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::{WhereClause, parse_quote};

use crate::field::{MetaMap, TreeField, TreeTrait};

fn get_doc(attrs: &[syn::Attribute]) -> Result<Option<String>> {
    fn doc_line(attr: &syn::Attribute) -> Result<String> {
        let syn::Meta::NameValue(meta) = &attr.meta else {
            return Err(Error::custom("Unexpected `doc` attribute format").with_span(&attr.meta));
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(doc),
            ..
        }) = &meta.value
        else {
            return Err(Error::custom("Unexpected `doc` attribute format").with_span(&meta.value));
        };
        Ok(doc.value().trim().to_string())
    }

    let mut docs = attrs.iter().filter(|attr| attr.path().is_ident("doc"));
    let Some(attr) = docs.next() else {
        return Ok(None);
    };
    let mut doc = doc_line(attr)?;
    for attr in docs {
        doc.push('\n');
        doc.push_str(&doc_line(attr)?);
    }
    Ok(Some(doc))
}

fn meta_get<'a>(meta: &'a MetaMap, name: &str) -> Option<&'a Override<SpannedValue<String>>> {
    meta.iter()
        .find_map(|(key, value)| (key == name).then_some(value))
}

fn meta_is_inherit(meta: &MetaMap, name: &str) -> bool {
    matches!(meta_get(meta, name), Some(Override::Inherit))
}

fn meta_insert(meta: &mut MetaMap, name: &str, value: Override<String>) {
    let value = match value {
        Override::Inherit => Override::Inherit,
        Override::Explicit(value) => {
            Override::Explicit(SpannedValue::new(value, Span::call_site()))
        }
    };
    meta.insert(syn::Ident::new(name, Span::call_site()), value);
}

fn meta_remove(meta: &mut MetaMap, name: &str) {
    meta.remove(&syn::Ident::new(name, Span::call_site()));
}

fn doc_to_meta(attrs: &[syn::Attribute], meta: &mut MetaMap, force: bool) -> Result<()> {
    if meta_is_inherit(meta, "doc") || (meta_get(meta, "doc").is_none() && force) {
        if let Some(doc) = get_doc(attrs)? {
            meta_insert(meta, "doc", Override::Explicit(doc));
        } else {
            meta_remove(meta, "doc");
        }
    }
    if meta_is_inherit(meta, "nullable") {
        meta_insert(meta, "nullable", Override::Explicit("true".to_string()));
    }
    for (k, v) in meta.iter() {
        if !v.is_explicit() {
            return Err(
                Error::custom(format!("'{k}' is not supported as inherited meta")).with_span(k),
            );
        }
    }
    Ok(())
}

fn meta_to_tokens(meta: &MetaMap) -> TokenStream {
    if !meta.is_empty() {
        let meta: TokenStream = meta
            .iter()
            .map(|(key, v)| {
                let v = v.as_ref().explicit().unwrap(); // All inherited meta have been converted
                let value: &String = v.as_ref();
                let key_span = key.span();
                let key = key.to_string();
                quote_spanned!(key_span=> (#key, #value), )
            })
            .collect();
        return quote!(::miniconf::Meta::new(&[#meta]));
    }
    quote!(::miniconf::Meta::EMPTY)
}

fn sem_to_tokens(oneof: bool) -> TokenStream {
    if oneof {
        quote!(::miniconf::ONEOF_SEM)
    } else {
        quote!(::miniconf::Sem::EMPTY)
    }
}

#[derive(Debug, FromVariant, Clone)]
#[darling(
    attributes(tree),
    forward_attrs(doc),
    and_then=Self::parse)]
struct TreeVariant {
    ident: syn::Ident,
    rename: Option<syn::Ident>,
    skip: Flag,
    fields: ast::Fields<TreeField>,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    meta: MetaMap,
}

impl TreeVariant {
    fn parse(mut self) -> Result<Self> {
        if self.fields.is_struct() {
            return Err(Error::custom(
                "Only newtype (single field tuple) and unit enum variants are supported.",
            )
            .with_span(&self.ident));
        }
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
}

#[derive(Debug, FromDeriveInput, Clone)]
#[darling(
    attributes(tree),
    forward_attrs(doc),
    supports(struct_named, struct_newtype, struct_tuple, enum_any),
    and_then=Self::parse)]
pub struct Tree {
    ident: syn::Ident,
    generics: syn::Generics,
    flatten: Flag,
    data: Data<TreeVariant, TreeField>,
    attrs: Vec<syn::Attribute>,
    #[darling(default)]
    meta: MetaMap,
}

impl Tree {
    fn no_leaf_error(span: &impl quote::ToTokens) -> Error {
        Error::custom("Internal nodes must have at least one leaf").with_span(span)
    }

    fn no_leaf_skip_error(span: Span) -> Error {
        let skip = syn::Ident::new("skip", span);
        Error::custom("Internal nodes must have at least one leaf").with_span(&skip)
    }

    fn parse(mut self) -> Result<Self> {
        match &mut self.data {
            Data::Struct(fields) => Self::parse_struct(fields)?,
            Data::Enum(variants) => Self::parse_enum(variants)?,
        }
        if self.flatten.is_present() && self.fields().len() != 1 {
            return Err(Error::custom("Can't flatten multiple fields/variants")
                .with_span(&self.flatten.span()));
        }
        if self.fields().is_empty() {
            return Err(Self::no_leaf_error(&self.ident));
        }
        self.fill_inherit_meta()?;
        Ok(self)
    }

    fn parse_struct(fields: &mut ast::Fields<TreeField>) -> Result<()> {
        let skip = fields
            .fields
            .iter()
            .find(|f| f.skip.is_present())
            .map(|f| f.skip);
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
        if let Some(field) = fields.fields.iter().find(|f| f.skip.is_present()) {
            return Err(
                Error::custom("Can only `skip` terminal tuple struct fields")
                    .with_span(&field.skip.span()),
            );
        }
        if fields.fields.is_empty()
            && let Some(skip) = skip
        {
            return Err(Self::no_leaf_skip_error(skip.span()));
        }
        Ok(())
    }

    fn parse_enum(variants: &mut Vec<TreeVariant>) -> Result<()> {
        let skip = variants
            .iter()
            .find(|variant| variant.skip.is_present())
            .map(|variant| variant.skip);
        variants.retain(|variant| !variant.skip.is_present() && !variant.fields.is_empty());
        if variants.is_empty()
            && let Some(skip) = skip
        {
            return Err(Self::no_leaf_skip_error(skip.span()));
        }
        for variant in variants.iter() {
            if variant.fields.len() != 1 {
                return Err(Error::custom(
                    "Only newtype (single field tuple) and unit enum variants are supported.",
                )
                .with_span(&variant.ident.span()));
            }
            if !variant.field().meta.is_empty() {
                let meta = variant.field().meta.first_key_value().map(|(k, _)| k);
                return Err(Error::custom(
                    "Node metadata must be placed on the variant, not the tuple field. Tuple fields only support edge metadata.",
                )
                .with_span(meta.unwrap_or(&variant.ident)));
            }
        }
        Ok(())
    }

    fn fill_inherit_meta(&mut self) -> Result<()> {
        if meta_is_inherit(&self.meta, "typename") {
            meta_insert(
                &mut self.meta,
                "typename",
                Override::Explicit(self.ident.to_string()),
            );
        }
        let force = meta_is_inherit(&self.meta, "doc");
        doc_to_meta(&self.attrs, &mut self.meta, false)?;
        match &mut self.data {
            Data::Struct(fields) => Self::fill_struct_meta(fields, force)?,
            Data::Enum(variants) => Self::fill_enum_meta(variants, force)?,
        }
        Ok(())
    }

    fn fill_struct_meta(fields: &mut ast::Fields<TreeField>, force: bool) -> Result<()> {
        for field in fields.fields.iter_mut() {
            doc_to_meta(&field.attrs, &mut field.meta, force)?;
        }
        Ok(())
    }

    fn fill_enum_meta(variants: &mut [TreeVariant], force: bool) -> Result<()> {
        for variant in variants.iter_mut() {
            doc_to_meta(&variant.attrs, &mut variant.meta, force)?;
            let field = variant.fields.fields.first_mut().unwrap();
            doc_to_meta(&field.attrs, &mut field.meta, force)?;
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
                        quote_spanned!(f.span()=> #i => #rhs)
                    })
                    .collect(),
                // TODO: Use the serde approach of a private enum (visitor) to get rid of the default
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
                        quote_spanned!(v.field().span()=> (Self::#ident(value, ..), #i) => #rhs)
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

    fn key_setup(&self) -> TokenStream {
        if self.flatten.is_present() {
            TokenStream::new()
        } else {
            quote!(let mut keys = keys;)
        }
    }

    fn flattened_schema(&self) -> TokenStream {
        let fields = self.fields();
        let field = fields.first().unwrap();
        if field.meta.is_empty() {
            field.schema()
        } else {
            let schema = field.schema();
            let meta = meta_to_tokens(&field.meta);
            quote_spanned! { field.span()=> {
                let schema = #schema;
                let sem = match schema.sem() {
                    ::core::option::Option::Some(sem) => *sem,
                    ::core::option::Option::None => ::miniconf::Sem::EMPTY,
                };
                &schema.rebuild(#meta, sem)
            }}
        }
    }

    fn schema_internal(&self) -> TokenStream {
        match &self.data {
            Data::Struct(fields) => self.struct_internal(fields),
            Data::Enum(variants) => self.enum_internal(variants),
        }
    }

    fn struct_internal(&self, fields: &ast::Fields<TreeField>) -> TokenStream {
        match fields.style {
            Style::Tuple => {
                let numbered: TokenStream = fields
                    .iter()
                    .map(|field| {
                        let schema = field.schema();
                        let meta = meta_to_tokens(&field.meta);
                        quote_spanned! { field.span()=> ::miniconf::Numbered::new(#schema, #meta), }
                    })
                    .collect();
                quote! { ::miniconf::Internal::Numbered(&[#numbered]) }
            }
            Style::Struct => {
                let named: TokenStream = fields
                    .iter()
                    .map(|field| {
                        let name = field.name().unwrap();
                        let schema = field.schema();
                        let meta = meta_to_tokens(&field.meta);
                        quote_spanned! { name.span()=> ::miniconf::Named::new(stringify!(#name), #schema, #meta), }
                    })
                    .collect();
                quote! { ::miniconf::Internal::Named(&[#named]) }
            }
            Style::Unit => unreachable!(),
        }
    }

    fn enum_internal(&self, variants: &[TreeVariant]) -> TokenStream {
        let named: TokenStream = variants
            .iter()
            .map(|variant| {
                let name = variant.name();
                let schema = variant.field().schema();
                let meta = meta_to_tokens(&variant.meta);
                quote_spanned! { variant.field().span()=> ::miniconf::Named::new(stringify!(#name), #schema, #meta), }
            })
            .collect();
        quote! { ::miniconf::Internal::Named(&[#named]) }
    }

    pub fn tree_schema(&self) -> TokenStream {
        let ident = &self.ident;
        let (impl_generics, ty_generics, orig_where_clause) = self.generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Schema, orig_where_clause);
        let schema = if self.flatten.is_present() {
            self.flattened_schema()
        } else {
            let internal = self.schema_internal();
            let meta = meta_to_tokens(&self.meta);
            let sem = sem_to_tokens(matches!(self.data, Data::Enum(_)));
            quote_spanned! { ident.span()=>
                &::miniconf::Schema::Internal(::miniconf::InternalSchema::new(
                    ::miniconf::NodeSchema::new(#meta, #sem),
                    #internal,
                ))
            }
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
        let key_setup = self.key_setup();
        let index = self.index();
        let (mat, arms, default) = self.arms(|f, i| f.serialize_by_key(i));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeSerialize for #ident #ty_generics #where_clause {
                fn serialize_by_key<S: ::miniconf::Serializer>(
                    &self,
                    keys: impl ::miniconf::Keys,
                    ser: S
                ) -> ::core::result::Result<S::Ok, ::miniconf::SerdeError<S::Error>>
                {
                    #key_setup
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
                .collect::<LifetimeRefSet<'_>>()
                .into_iter()
                .cloned(),
        );
        let mut generics = self.generics.clone();
        generics.params.push(syn::GenericParam::Lifetime(de));
        let (impl_generics, _, where_clause) = generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Deserialize, where_clause);
        let key_setup = self.key_setup();
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
                    keys: impl ::miniconf::Keys,
                    de: D
                ) -> ::core::result::Result<(), ::miniconf::SerdeError<D::Error>>
                {
                    #key_setup
                    let index = #index?;
                    match #mat {
                        #(#deserialize_arms ,)*
                        _ => #default
                    }
                }

            fn probe_by_key<D: ::miniconf::Deserializer<'de>>(
                keys: impl ::miniconf::Keys,
                de: D
            ) -> ::core::result::Result<(), ::miniconf::SerdeError<D::Error>>
                {
                    #key_setup
                    let index = #index?;
                    match index {
                        #(#probe_arms ,)*
                        _ => #default
                    }
                }
            }
        }
    }

    pub fn tree_any(&self) -> TokenStream {
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();
        let where_clause = self.bound_generics(TreeTrait::Any, where_clause);
        let key_setup = self.key_setup();
        let index = self.index();
        let ident = &self.ident;
        let (mat, ref_arms, default) = self.arms(|f, i| f.ref_any_by_key(i));
        let (_, mut_arms, _) = self.arms(|f, i| f.mut_any_by_key(i));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeAny for #ident #ty_generics #where_clause {
                fn ref_any_by_key(
                    &self,
                    keys: impl ::miniconf::Keys
                ) -> ::core::result::Result<&dyn ::core::any::Any, ::miniconf::ValueError>
                {
                    #key_setup
                    let index = #index?;
                    match #mat {
                        #(#ref_arms ,)*
                        _ => #default
                    }
                }

                fn mut_any_by_key(
                    &mut self,
                    keys: impl ::miniconf::Keys
                ) -> ::core::result::Result<&mut dyn ::core::any::Any, ::miniconf::ValueError>
                {
                    #key_setup
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
