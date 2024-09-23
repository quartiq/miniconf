use darling::{
    ast::{self, Data},
    util::Flag,
    Error, FromDeriveInput, FromVariant,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse_quote;

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
}

#[derive(Debug, FromDeriveInput, Clone)]
#[darling(attributes(tree))]
#[darling(supports(any))]
pub struct Tree {
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    pub data: ast::Data<TreeVariant, TreeField>,
}

impl Tree {
    pub(crate) fn depth(&self) -> usize {
        match &self.data {
            Data::Struct(fields) => fields.fields.iter().fold(0, |d, field| d.max(field.depth)) + 1,
            Data::Enum(variants) => {
                variants
                    .iter()
                    .flat_map(|v| &v.fields.fields)
                    .fold(0, |d, field| d.max(field.depth))
                    + 1
            }
        }
    }

    pub(crate) fn parse(input: &syn::DeriveInput) -> Result<Self, Error> {
        let mut tree = Self::from_derive_input(input)?;

        match &mut tree.data {
            Data::Struct(fields) => {
                // unnamed fields can only be skipped if they are terminal
                while fields
                    .fields
                    .last()
                    .map(|f| f.skip.is_present())
                    .unwrap_or_default()
                {
                    fields.fields.pop();
                }
                fields
                    .fields
                    .retain(|f| f.ident.is_none() || !f.skip.is_present());
                if let Some(f) = fields.fields.iter().find(|f| f.skip.is_present()) {
                    return Err(
                        Error::custom("Can only `skip` terminal tuple struct fields")
                            .with_span(&f.skip.span()),
                    );
                }
            }
            Data::Enum(variants) => {
                variants.retain(|v| !v.skip.is_present());
                for v in variants.iter_mut() {
                    if !(v.fields.is_newtype() || v.fields.is_unit()) {
                        // Note(design) For tuple or named struct variants we'd have to create proxy
                        // structs anyway to support KeyLookup on that level.
                        return Err(Error::custom("Only newtype or unit variants are supported")
                            .with_span(&v.ident.span())); // FIXME: Fields.span is not pub, no span()
                    }
                    assert!(v.fields.len() <= 1);
                }
                variants.retain(|v| v.fields.is_newtype());
            }
        }
        Ok(tree)
    }

    pub(crate) fn bound_generics<F>(&self, func: &mut F) -> syn::Generics
    where
        F: FnMut(usize) -> Option<syn::TypeParamBound>,
    {
        let mut generics = self.generics.clone();
        match &self.data {
            Data::Struct(fields) => fields
                .fields
                .iter()
                .for_each(|f| walk_type_params(f.typ(), func, f.depth, &mut generics)),
            Data::Enum(variants) => variants
                .iter()
                .flat_map(|v| v.fields.fields.iter())
                .for_each(|f| walk_type_params(f.typ(), func, f.depth, &mut generics)),
        }
        generics
    }

    pub(crate) fn tree_key(&self) -> TokenStream {
        let depth = self.depth();
        let ident = &self.ident;
        let generics = self.bound_generics(&mut |depth| {
            (depth > 0).then_some(parse_quote!(::miniconf::TreeKey<#depth>))
        });
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        match &self.data {
            Data::Struct(fields) => {
                let fields = &fields.fields;
                let fields_len = fields.len();

                let (names, name_to_index, index_to_name, index_len) =
                    if fields.iter().all(|f| f.ident.is_none()) {
                        (
                            None,
                            quote!(str::parse(value).ok()),
                            quote!(if index >= #fields_len {
                                Err(::miniconf::Traversal::NotFound(1))?
                            } else {
                                None
                            }),
                            quote!(index.checked_ilog10().unwrap_or_default() as usize + 1),
                        )
                    } else {
                        let names = fields.iter().map(|field| {
                            // ident is Some
                            let name = field.name().unwrap();
                            quote! { stringify!(#name) }
                        });
                        (
                            Some(quote!(
                                const __MINICONF_NAMES: &'static [&'static str] = &[#(#names ,)*];
                            )),
                            quote!(Self::__MINICONF_NAMES.iter().position(|&n| n == value)),
                            quote!(Some(
                                *Self::__MINICONF_NAMES
                                    .get(index)
                                    .ok_or(::miniconf::Traversal::NotFound(1))?
                            )),
                            quote!(Self::__MINICONF_NAMES[index].len()),
                        )
                    };

                let traverse_by_key_arms = fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, field)| field.traverse_by_key(i));
                let metadata_arms = fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, field)| field.metadata(i));
                let defers = fields.iter().map(|field| field.depth > 0);

                quote! {
                    impl #impl_generics #ident #ty_generics #where_clause {
                        // TODO: can these be hidden and disambiguated w.r.t. collision?
                        fn __miniconf_lookup<K: ::miniconf::Keys>(keys: &mut K) -> Result<usize, ::miniconf::Traversal> {
                            const DEFERS: [bool; #fields_len] = [#(#defers ,)*];
                            let index = ::miniconf::Keys::next::<Self>(keys)?;
                            let defer = DEFERS.get(index)
                                .ok_or(::miniconf::Traversal::NotFound(1))?;
                            if !defer && !keys.finalize() {
                                Err(::miniconf::Traversal::TooLong(1))
                            } else {
                                Ok(index)
                            }
                        }

                        #names
                    }

                    #[automatically_derived]
                    impl #impl_generics ::miniconf::KeyLookup for #ident #ty_generics #where_clause {
                        const LEN: usize = #fields_len;

                        #[inline]
                        fn name_to_index(value: &str) -> Option<usize> {
                            #name_to_index
                        }
                    }

                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeKey<#depth> for #ident #ty_generics #where_clause {
                        fn metadata() -> ::miniconf::Metadata {
                            let mut meta = ::miniconf::Metadata::default();
                            for index in 0..#fields_len {
                                let item_meta: ::miniconf::Metadata = match index {
                                    #(#metadata_arms ,)*
                                    _ => {
                                        let mut m = ::miniconf::Metadata::default();
                                        m.count = 1;
                                        m
                                    }
                                };
                                meta.max_length = meta.max_length.max(
                                    #index_len +
                                    item_meta.max_length
                                );
                                meta.max_depth = meta.max_depth.max(
                                    item_meta.max_depth
                                );
                                meta.count += item_meta.count;
                            }
                            meta.max_depth += 1;
                            meta
                        }

                        fn traverse_by_key<K, F, E>(
                            mut keys: K,
                            mut func: F,
                        ) -> Result<usize, ::miniconf::Error<E>>
                        where
                            K: ::miniconf::Keys,
                            F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
                        {
                            let index = ::miniconf::Keys::next::<Self>(&mut keys)?;
                            let name = #index_to_name;
                            func(index, name, #fields_len).map_err(|err| ::miniconf::Error::Inner(1, err))?;
                            ::miniconf::Error::increment_result(match index {
                                #(#traverse_by_key_arms ,)*
                                _ => {
                                    if !keys.finalize() {
                                        Err(::miniconf::Traversal::TooLong(0).into())
                                    } else {
                                        Ok(0)
                                    }
                                }
                            })
                        }
                    }
                }
            }
            Data::Enum(variants) => {
                let variants_len = variants.len();
                let names = variants.iter().map(|v| {
                    let name = v.name();
                    quote! { stringify!(#name) }
                });
                let defers = variants.iter().map(|v| v.field().depth > 0);

                let metadata_arms = variants
                    .iter()
                    .enumerate()
                    .filter_map(|(i, variant)| variant.field().metadata(i));
                let traverse_by_key_arms = variants
                    .iter()
                    .enumerate()
                    .filter_map(|(i, variant)| variant.field().traverse_by_key(i));

                quote! {
                    impl #impl_generics #ident #ty_generics #where_clause {
                        // TODO: can these be hidden and disambiguated w.r.t. collision?
                        fn __miniconf_lookup<K: ::miniconf::Keys>(keys: &mut K) -> Result<usize, ::miniconf::Traversal> {
                            const DEFERS: [bool; #variants_len] = [#(#defers ,)*];
                            let index = ::miniconf::Keys::next::<Self>(keys)?;
                            let defer = DEFERS.get(index)
                                .ok_or(::miniconf::Traversal::NotFound(1))?;
                            if !defer && !keys.finalize() {
                                Err(::miniconf::Traversal::TooLong(1))
                            } else {
                                Ok(index)
                            }
                        }

                        const __MINICONF_NAMES: [&'static str; #variants_len] = [#(#names ,)*];
                    }

                    #[automatically_derived]
                    impl #impl_generics ::miniconf::KeyLookup for #ident #ty_generics #where_clause {
                        const LEN: usize = #variants_len;

                        #[inline]
                        fn name_to_index(value: &str) -> Option<usize> {
                            Self::__MINICONF_NAMES.iter().position(|&n| n == value)
                        }
                    }

                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeKey<#depth> for #ident #ty_generics #where_clause {
                        fn metadata() -> ::miniconf::Metadata {
                            let mut meta = ::miniconf::Metadata::default();
                            for index in 0..#variants_len {
                                let item_meta: ::miniconf::Metadata = match index {
                                    #(#metadata_arms ,)*
                                    _ => {
                                        let mut m = ::miniconf::Metadata::default();
                                        m.count = 1;
                                        m
                                    }
                                };
                                meta.max_length = meta.max_length.max(
                                    Self::__MINICONF_NAMES[index].len() +
                                    item_meta.max_length
                                );
                                meta.max_depth = meta.max_depth.max(
                                    item_meta.max_depth
                                );
                                meta.count += item_meta.count;
                            }
                            meta.max_depth += 1;
                            meta
                        }

                        fn traverse_by_key<K, F, E>(
                            mut keys: K,
                            mut func: F,
                        ) -> Result<usize, ::miniconf::Error<E>>
                        where
                            K: ::miniconf::Keys,
                            F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
                        {
                            let index = ::miniconf::Keys::next::<Self>(&mut keys)?;
                            let name = Some(*Self::__MINICONF_NAMES
                                .get(index)
                                .ok_or(::miniconf::Traversal::NotFound(1))?);
                            func(index, name, #variants_len).map_err(|err| ::miniconf::Error::Inner(1, err))?;
                            ::miniconf::Error::increment_result(match index {
                                #(#traverse_by_key_arms ,)*
                                _ => {
                                    if !keys.finalize() {
                                        Err(::miniconf::Traversal::TooLong(0).into())
                                    } else {
                                        Ok(0)
                                    }
                                }
                            })
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn tree_serialize(&self) -> TokenStream {
        let depth = self.depth();
        let ident = &self.ident;
        let generics = self.bound_generics(&mut |depth| {
            if depth > 0 {
                Some(parse_quote!(::miniconf::TreeSerialize<#depth>))
            } else {
                Some(parse_quote!(::miniconf::Serialize))
            }
        });
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        match &self.data {
            Data::Struct(fields) => {
                let serialize_by_key_arms = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| field.serialize_by_key(i, None));

                quote! {
                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeSerialize<#depth> for #ident #ty_generics #where_clause {
                        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, ::miniconf::Error<S::Error>>
                        where
                            K: ::miniconf::Keys,
                            S: ::miniconf::Serializer,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            ::miniconf::Error::increment_result(match index {
                                #(#serialize_by_key_arms ,)*
                                _ => unreachable!(),
                            })
                        }
                    }
                }
            }
            Data::Enum(variants) => {
                let serialize_by_key_arms = variants
                    .iter()
                    .enumerate()
                    .map(|(i, variant)| variant.field().serialize_by_key(i, Some(&variant.ident)));

                quote! {
                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeSerialize<#depth> for #ident #ty_generics #where_clause {
                        fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> Result<usize, ::miniconf::Error<S::Error>>
                        where
                            K: ::miniconf::Keys,
                            S: ::miniconf::Serializer,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            ::miniconf::Error::increment_result(match (self, index) {
                                #(#serialize_by_key_arms ,)*
                                _ => Err(::miniconf::Traversal::Absent(0).into()),
                            })
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn tree_deserialize(&self) -> TokenStream {
        let mut generics = self.bound_generics(&mut |depth| {
            if depth > 0 {
                Some(parse_quote!(::miniconf::TreeDeserialize<'de, #depth>))
            } else {
                Some(parse_quote!(::miniconf::Deserialize<'de>))
            }
        });

        let depth = self.depth();
        let ident = &self.ident;

        let orig_generics = generics.clone();
        let (_, ty_generics, where_clause) = orig_generics.split_for_impl();
        let lts: Vec<_> = generics.lifetimes().cloned().collect();
        generics.params.push(parse_quote!('de));
        if let Some(syn::GenericParam::Lifetime(de)) = generics.params.last_mut() {
            assert_eq!(de.lifetime.ident, "de");
            for l in lts {
                assert!(l.lifetime.ident != "de");
                de.bounds.push(l.lifetime);
            }
        }
        let (impl_generics, _, _) = generics.split_for_impl();

        match &self.data {
            Data::Struct(fields) => {
                let deserialize_by_key_arms = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| field.deserialize_by_key(i, None));

                quote! {
                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeDeserialize<'de, #depth> for #ident #ty_generics #where_clause {
                        fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, ::miniconf::Error<D::Error>>
                        where
                            K: ::miniconf::Keys,
                            D: ::miniconf::Deserializer<'de>,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            ::miniconf::Error::increment_result(match index {
                                #(#deserialize_by_key_arms ,)*
                                _ => unreachable!(),
                            })
                        }
                    }
                }
            }
            Data::Enum(variants) => {
                let deserialize_by_key_arms = variants.iter().enumerate().map(|(i, variant)| {
                    variant.field().deserialize_by_key(i, Some(&variant.ident))
                });

                quote! {
                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeDeserialize<'de, #depth> for #ident #ty_generics #where_clause {
                        fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> Result<usize, ::miniconf::Error<D::Error>>
                        where
                            K: ::miniconf::Keys,
                            D: ::miniconf::Deserializer<'de>,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            ::miniconf::Error::increment_result(match (self, index) {
                                #(#deserialize_by_key_arms ,)*
                                _ => Err(::miniconf::Traversal::Absent(0).into()),
                            })
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn tree_any(&self) -> TokenStream {
        let generics = self.bound_generics(&mut |depth| {
            if depth > 0 {
                Some(parse_quote!(::miniconf::TreeAny<#depth>))
            } else {
                Some(parse_quote!(::core::any::Any))
            }
        });

        let depth = self.depth();
        let ident = &self.ident;
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        match &self.data {
            Data::Struct(fields) => {
                let ref_any_by_key_arms = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| field.ref_any_by_key(i, None));
                let mut_any_by_key_arms = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| field.mut_any_by_key(i, None));

                quote! {
                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeAny<#depth> for #ident #ty_generics #where_clause {
                        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn ::core::any::Any, ::miniconf::Traversal>
                        where
                            K: ::miniconf::Keys,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            {
                                let ret: Result<_, _> = match index {
                                    #(#ref_any_by_key_arms ,)*
                                    _ => unreachable!()
                                };
                                ret.map_err(::miniconf::Traversal::increment)
                            }
                        }

                        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn ::core::any::Any, ::miniconf::Traversal>
                        where
                            K: ::miniconf::Keys,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            {
                                let ret: Result<_, _> = match index {
                                    #(#mut_any_by_key_arms ,)*
                                    _ => unreachable!()
                                };
                                ret.map_err(::miniconf::Traversal::increment)
                            }
                        }
                    }
                }
            }
            Data::Enum(variants) => {
                let ref_any_by_key_arms = variants
                    .iter()
                    .enumerate()
                    .map(|(i, variant)| variant.field().ref_any_by_key(i, Some(&variant.ident)));
                let mut_any_by_key_arms = variants
                    .iter()
                    .enumerate()
                    .map(|(i, variant)| variant.field().mut_any_by_key(i, Some(&variant.ident)));

                quote! {
                    #[automatically_derived]
                    impl #impl_generics ::miniconf::TreeAny<#depth> for #ident #ty_generics #where_clause {
                        fn ref_any_by_key<K>(&self, mut keys: K) -> Result<&dyn ::core::any::Any, ::miniconf::Traversal>
                        where
                            K: ::miniconf::Keys,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            {
                                let ret: Result<_, _> = match index {
                                    #(#ref_any_by_key_arms ,)*
                                    _ => Err(::miniconf::Traversal::Absent(0).into()),
                                };
                                ret.map_err(::miniconf::Traversal::increment)
                            }
                        }

                        fn mut_any_by_key<K>(&mut self, mut keys: K) -> Result<&mut dyn ::core::any::Any, ::miniconf::Traversal>
                        where
                            K: ::miniconf::Keys,
                        {
                            let index = Self::__miniconf_lookup(&mut keys)?;
                            // Note(unreachable) empty structs have diverged by now
                            #[allow(unreachable_code)]
                            {
                                let ret: Result<_, _> = match index {
                                    #(#mut_any_by_key_arms ,)*
                                    _ => Err(::miniconf::Traversal::Absent(0).into()),
                                };
                                ret.map_err(::miniconf::Traversal::increment)
                            }
                        }
                    }
                }
            }
        }
    }
}

fn walk_type_params<F>(typ: &syn::Type, func: &mut F, depth: usize, generics: &mut syn::Generics)
where
    F: FnMut(usize) -> Option<syn::TypeParamBound>,
{
    match typ {
        syn::Type::Path(syn::TypePath { path, .. }) => {
            if let Some(ident) = path.get_ident() {
                // The type is a single ident (no other path segments, has no generics):
                // call back if it is a generic type for us
                for generic in &mut generics.params {
                    if let syn::GenericParam::Type(type_param) = generic {
                        if &type_param.ident == ident {
                            if let Some(bound) = func(depth) {
                                type_param.bounds.push(bound);
                            }
                        }
                    }
                }
            } else {
                // Analyze the type parameters of the type, as they may be generics for us as well
                // This tries to reproduce the bounds that field types place on
                // their generic types, directly or indirectly. For this the API depth (the const generic
                // param to `TreeKey<Y>` etc) is determined as follows:
                //
                // Assume that all types use their generic T at
                // relative depth 1, i.e.
                // * if `#[tree(depth(Y > 1))] a: S<T>` then `T: Tree{Key,Serialize,Deserialize}<Y - 1>`
                // * else (that is if `Y = 1` or `a: S<T>` without `#[tree]`) then
                //   `T: serde::{Serialize,Deserialize}`
                //
                // And analogously for nested types `S<T<U>>` and `[[T; ..]; ..]` etc.
                // This is correct for all types in this library (Option, array, structs with the derive macro).
                //
                // The bounds are conservative (might not be required) and
                // fragile (might apply the wrong bound).
                // This matches the standard derive behavior and its issues
                // https://github.com/rust-lang/rust/issues/26925
                //
                // To fix this, one would extend the attribute syntax to allow overriding bounds.
                for seg in path.segments.iter() {
                    if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                        for arg in args.args.iter() {
                            if let syn::GenericArgument::Type(typ) = arg {
                                // Found type argument in field type: recurse
                                walk_type_params(typ, func, depth.saturating_sub(1), generics);
                            }
                        }
                    }
                }
            }
        }
        syn::Type::Array(syn::TypeArray { elem, .. })
        | syn::Type::Slice(syn::TypeSlice { elem, .. }) => {
            // An array or slice places the element exactly one level deeper: recurse.
            walk_type_params(elem, func, depth.saturating_sub(1), generics);
        }
        syn::Type::Reference(syn::TypeReference { elem, .. }) => {
            // A reference is transparent
            walk_type_params(elem, func, depth, generics);
        }
        other => panic!("Unsupported type: {:?}", other),
    };
}
