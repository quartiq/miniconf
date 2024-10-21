use darling::{
    ast::{self, Data},
    util::Flag,
    Error, FromDeriveInput, FromVariant,
};
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::parse_quote;

use crate::field::TreeField;

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
        // unnamed fields can only be skipped if they are terminal
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
                Error::custom("Can only `skip` terminal tuple struct fields")
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
#[darling(attributes(tree), supports(any), and_then=Self::parse)]
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
                // unnamed fields can only be skipped if they are terminal
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
                        // This could be lifted with a index map.
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
                    ::miniconf::Traversal::Absent(0).into()
                )),
            ),
        }
    }

    fn bound_generics(&self, bound: syn::TraitBound) -> syn::Generics {
        let mut generics = self.generics.clone();
        // println!("BOUND {:?}", &generics);
        for f in self.fields() {
            walk_type_params(f.typ(), &bound, &mut generics);
        }
        generics
    }

    pub fn tree_key(&self) -> TokenStream {
        let ident = &self.ident;
        let generics = self.bound_generics(parse_quote!(::miniconf::TreeKey));
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let fields = self.fields();
        let fields_len = fields.len();
        let traverse_all_arms = fields.iter().enumerate().map(|(i, f)| {
            let w = f.traverse_all();
            if self.flatten.is_present() {
                quote!(walk = #w;)
            } else {
                quote!(walk = walk.merge(&#w, Some(#i), &Self::__MINICONF_LOOKUP)?;)
            }
        });
        let traverse_arms = fields.iter().enumerate().map(|(i, f)| f.traverse_by_key(i));
        let names: Option<Vec<_>> = match &self.data {
            Data::Struct(fields) if fields.style.is_struct() => Some(
                fields
                    .iter()
                    .map(|f| {
                        // ident is Some
                        let name = f.name().unwrap();
                        quote_spanned! { name.span()=> stringify!(#name) }
                    })
                    .collect(),
            ),
            Data::Enum(variants) => Some(
                variants
                    .iter()
                    .map(|v| {
                        let name = v.name();
                        quote_spanned! { name.span()=> stringify!(#name) }
                    })
                    .collect(),
            ),
            _ => None,
        };
        let names = match names {
            None => quote!(::core::option::Option::None),
            Some(names) => quote!(::core::option::Option::Some(&[#(#names ,)*])),
        };

        let (index, traverse, increment) = if self.flatten.is_present() {
            (quote!(0), None, None)
        } else {
            (
                quote!(::miniconf::Keys::next(&mut keys, &Self::__MINICONF_LOOKUP)?),
                Some(quote! {
                    let name = Self::__MINICONF_LOOKUP.lookup(index)?;
                    func(index, name, Self::__MINICONF_LOOKUP.len)
                    .map_err(|err| ::miniconf::Error::Inner(1, err))?;
                }),
                Some(quote!(::miniconf::Error::increment_result)),
            )
        };

        quote! {
            // TODO: can these be hidden and disambiguated w.r.t. collision?
            #[automatically_derived]
            impl #impl_generics #ident #ty_generics #where_clause {
                const __MINICONF_LOOKUP: ::miniconf::KeyLookup = ::miniconf::KeyLookup {
                    len: #fields_len,
                    names: #names,
                };

                fn __miniconf_lookup<K: ::miniconf::Keys>(mut keys: K) -> ::core::result::Result<usize, ::miniconf::Traversal> {
                    let index = #index;
                    if index >= Self::__MINICONF_LOOKUP.len {
                        ::core::result::Result::Err(::miniconf::Traversal::NotFound(1))
                    } else {
                        ::core::result::Result::Ok(index)
                    }
                }
            }

            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeKey for #ident #ty_generics #where_clause {
                fn traverse_all<W: ::miniconf::Walk>() -> ::core::result::Result<W, W::Error> {
                    #[allow(unused_mut)]
                    let mut walk = W::internal();
                    #(#traverse_all_arms)*
                    ::core::result::Result::Ok(walk)
                }

                fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> ::core::result::Result<usize, ::miniconf::Error<E>>
                where
                    K: ::miniconf::Keys,
                    F: ::core::ops::FnMut(usize, ::core::option::Option<&'static str>, usize) -> ::core::result::Result<(), E>,
                {
                    let index = #index;
                    #traverse
                    #[allow(unreachable_code)]
                    #increment(match index {
                        #(#traverse_arms ,)*
                        _ => unreachable!()
                    })
                }
            }
        }
    }

    pub fn tree_serialize(&self) -> TokenStream {
        let ident = &self.ident;
        let generics = self.bound_generics(parse_quote!(::miniconf::TreeSerialize));

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let (mat, arms, default) = self.arms(|f, i| f.serialize_by_key(i));
        let increment =
            (!self.flatten.is_present()).then_some(quote!(::miniconf::Error::increment_result));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeSerialize for #ident #ty_generics #where_clause {
                fn serialize_by_key<K, S>(&self, mut keys: K, ser: S) -> ::core::result::Result<usize, ::miniconf::Error<S::Error>>
                where
                    K: ::miniconf::Keys,
                    S: ::miniconf::Serializer,
                {
                    let index = Self::__miniconf_lookup(&mut keys)?;
                    // Note(unreachable) empty structs have diverged by now
                    #[allow(unreachable_code)]
                    #increment(match #mat {
                        #(#arms ,)*
                        _ => #default
                    })
                }
            }
        }
    }

    pub fn tree_deserialize(&self) -> TokenStream {
        let mut generics = self.bound_generics(parse_quote!(::miniconf::TreeDeserialize<'de>));

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
        let (mat, arms, default) = self.arms(|f, i| f.deserialize_by_key(i));
        let increment =
            (!self.flatten.is_present()).then_some(quote!(::miniconf::Error::increment_result));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeDeserialize<'de> for #ident #ty_generics #where_clause {
                fn deserialize_by_key<K, D>(&mut self, mut keys: K, de: D) -> ::core::result::Result<usize, ::miniconf::Error<D::Error>>
                where
                    K: ::miniconf::Keys,
                    D: ::miniconf::Deserializer<'de>,
                {
                    let index = Self::__miniconf_lookup(&mut keys)?;
                    // Note(unreachable) empty structs have diverged by now
                    #[allow(unreachable_code)]
                    #increment(match #mat {
                        #(#arms ,)*
                        _ => #default
                    })
                }
            }
        }
    }

    pub fn tree_any(&self) -> TokenStream {
        let generics = self.bound_generics(parse_quote!(::miniconf::TreeAny));

        let ident = &self.ident;
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let (mat, ref_arms, default) = self.arms(|f, i| f.ref_any_by_key(i));
        let (_, mut_arms, _) = self.arms(|f, i| f.mut_any_by_key(i));
        let increment = (!self.flatten.is_present())
            .then_some(quote!(.map_err(::miniconf::Traversal::increment)));

        quote! {
            #[automatically_derived]
            impl #impl_generics ::miniconf::TreeAny for #ident #ty_generics #where_clause {
                fn ref_any_by_key<K>(&self, mut keys: K) -> ::core::result::Result<&dyn ::core::any::Any, ::miniconf::Traversal>
                where
                    K: ::miniconf::Keys,
                {
                    let index = Self::__miniconf_lookup(&mut keys)?;
                    // Note(unreachable) empty structs have diverged by now
                    #[allow(unreachable_code)]
                    {
                        let ret: ::core::result::Result<_, _> = match #mat {
                            #(#ref_arms ,)*
                            _ => #default
                        };
                        ret #increment
                    }
                }

                fn mut_any_by_key<K>(&mut self, mut keys: K) -> ::core::result::Result<&mut dyn ::core::any::Any, ::miniconf::Traversal>
                where
                    K: ::miniconf::Keys,
                {
                    let index = Self::__miniconf_lookup(&mut keys)?;
                    // Note(unreachable) empty structs have diverged by now
                    #[allow(unreachable_code)]
                    {
                        let ret: ::core::result::Result<_, _> = match #mat {
                            #(#mut_arms ,)*
                            _ => #default
                        };
                        ret #increment
                    }
                }
            }
        }
    }
}

fn walk_type_params(typ: &syn::Type, bound: &syn::TraitBound, generics: &mut syn::Generics) {
    match typ {
        syn::Type::Path(syn::TypePath { path, .. }) => {
            if let Some(ident) = path.get_ident() {
                // The type is a single ident (no other path segments, has no generics):
                // call back if it is a generic type for us
                for generic in &mut generics.params {
                    if let syn::GenericParam::Type(type_param) = generic {
                        if &type_param.ident == ident {
                            type_param.bounds.push(bound.clone().into());
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
                                if seg.ident == "Leaf" {
                                    // let syn::Type::Path(syn::TypePath { path, .. }) = typ else {
                                    //     unimplemented!()
                                    // };
                                    // println!("{:?}", typ);
                                    // let id = path.get_ident().unwrap();
                                    // generics
                                    //     .where_clause
                                    //     .get_or_insert(parse_quote!(where Leaf<#id>: #bound));
                                } else {
                                    walk_type_params(typ, bound, generics);
                                }
                            }
                        }
                    }
                }
            }
        }
        syn::Type::Array(syn::TypeArray { elem, .. })
        | syn::Type::Slice(syn::TypeSlice { elem, .. })
        | syn::Type::Reference(syn::TypeReference { elem, .. }) => {
            walk_type_params(elem, bound, generics);
        }
        syn::Type::Tuple(syn::TypeTuple { elems, .. }) => {
            for elem in elems.iter() {
                walk_type_params(elem, bound, generics);
            }
        }
        other => panic!("Unsupported type: {:?}", other),
    };
}
