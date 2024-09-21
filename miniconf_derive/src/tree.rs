use darling::{
    ast::{self, Data},
    util::{Flag, SpannedValue},
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
    pub skip: Flag,
    pub fields: ast::Fields<TreeField>,
}

#[derive(Debug, FromDeriveInput, Clone)]
#[darling(attributes(tree))]
#[darling(supports(any))]
pub struct Tree {
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    // pub vis: syn::Visibility,
    pub data: ast::Data<TreeVariant, TreeField>,
    // attrs: Vec<syn::Attribute>,
    pub tag: Option<SpannedValue<syn::Path>>, // FIXME: implement
}

impl Tree {
    pub(crate) fn depth(&self) -> usize {
        match &self.data {
            Data::Struct(fields) => depth(&fields.fields) + 1,
            Data::Enum(variants) => depth(variants.iter().flat_map(|v| &v.fields.fields)) + 2,
        }
    }

    pub(crate) fn parse(input: &syn::DeriveInput) -> Result<Self, Error> {
        let mut tree = Self::from_derive_input(input)?;

        match &mut tree.data {
            Data::Struct(fields) => {
                if let Some(tag) = &tree.tag {
                    return Err(Error::custom("No `tag` for structs").with_span(&tag.span()));
                }
                remove_skipped(&mut fields.fields)?;
            }
            Data::Enum(variants) => {
                if tree.tag.is_some() {
                    unimplemented!();
                }
                variants.retain(|v| !v.skip.is_present());
                for v in variants.iter_mut() {
                    remove_skipped(&mut v.fields.fields)?;
                    for f in v.fields.fields.iter_mut() {
                        f.variant = true;
                    }
                }
            }
        }
        Ok(tree)
    }

    pub(crate) fn fields(&self) -> &Vec<TreeField> {
        let Data::Struct(fields) = &self.data else {
            unimplemented!()
        };
        &fields.fields
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
            if depth > 0 {
                Some(parse_quote!(::miniconf::TreeKey<#depth>))
            } else {
                None
            }
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

                    impl #impl_generics ::miniconf::KeyLookup for #ident #ty_generics #where_clause {
                        const LEN: usize = #fields_len;

                        #[inline]
                        fn name_to_index(value: &str) -> Option<usize> {
                            #name_to_index
                        }
                    }

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
                unimplemented!()
            }
        }
    }
}

fn depth<'a>(fields: impl IntoIterator<Item = &'a TreeField>) -> usize {
    fields.into_iter().fold(0, |d, field| d.max(field.depth))
}

fn remove_skipped(fields: &mut Vec<TreeField>) -> Result<(), Error> {
    // unnamed fields can only be skipped if they are terminal
    while fields
        .last()
        .map(|f| f.ident.is_none() && f.skip.is_present())
        .unwrap_or_default()
    {
        fields.pop();
    }
    fields.retain(|f| f.ident.is_some() && !f.skip.is_present());
    if let Some(f) = fields.iter().filter(|f| f.skip.is_present()).next() {
        Err(
            Error::custom("Can not `skip` non-terminal tuple struct fields")
                .with_span(&f.skip.span()),
        )
    } else {
        Ok(())
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
