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
                            "Only newtype (single field tuple) enum variants are supported.",
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

    fn level(&self) -> usize {
        if self.flatten.is_present() {
            0
        } else {
            1
        }
    }

    fn depth(&self) -> usize {
        let inner = self.fields().iter().fold(0, |d, field| d.max(field.depth));
        (self.level() + inner).max(1) // We need to eat at least one level. C.f. impl TreeKey for Option.
    }

    fn fields(&self) -> Vec<&TreeField> {
        match &self.data {
            Data::Struct(fields) => fields.iter().collect(),
            Data::Enum(variants) => variants.iter().map(|v| v.field()).collect(),
        }
    }

    fn bound_generics<F>(&self, func: &mut F) -> syn::Generics
    where
        F: FnMut(usize) -> Option<syn::TraitBound>,
    {
        let mut generics = self.generics.clone();
        for f in self.fields() {
            walk_type_params(f.typ(), func, f.depth, &mut generics);
        }
        generics
    }

    pub fn tree_key(&self) -> TokenStream {
        let depth = self.depth();
        let ident = &self.ident;
        let generics = self.bound_generics(&mut |depth| {
            (depth > 0).then_some(parse_quote!(::miniconf::TreeKey<#depth>))
        });
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let fields = self.fields();
        let fields_len = fields.len();
        let metadata_arms = fields.iter().enumerate().map(|(i, f)| f.metadata(i));
        let traverse_arms = fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.traverse_by_key(i));
        let defers = fields.iter().map(|field| field.depth > 0);
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
        let (names, name_to_index, index_to_name, mut ident_len) = if let Some(names) = names {
            (
                Some(quote!(
                    const __MINICONF_NAMES: [&'static str; #fields_len] = [#(#names ,)*];
                )),
                quote!(Self::__MINICONF_NAMES.iter().position(|&n| n == value)),
                quote!(Some(
                    *Self::__MINICONF_NAMES
                        .get(index)
                        .ok_or(::miniconf::Traversal::NotFound(1))?
                )),
                quote!(Self::__MINICONF_NAMES[index].len()),
            )
        } else {
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
        };

        let level = self.level();
        let (index, traverse, increment) = if self.flatten.is_present() {
            ident_len = quote!(0);
            (quote!(0), quote!(), quote!())
        } else {
            (
                quote!(::miniconf::Keys::next::<Self>(&mut keys)?),
                quote! {
                    func(index, #index_to_name, #fields_len).map_err(|err| ::miniconf::Error::Inner(1, err))?;
                },
                quote!(::miniconf::Error::increment_result),
            )
        };

        // FIXME: don't increment result in flatten in other traits

        quote! {
            #[automatically_derived]
            impl #impl_generics #ident #ty_generics #where_clause {
                // TODO: can these be hidden and disambiguated w.r.t. collision?
                fn __miniconf_lookup<K: ::miniconf::Keys>(mut keys: K) -> Result<usize, ::miniconf::Traversal> {
                    const DEFERS: [bool; #fields_len] = [#(#defers ,)*];
                    let index = #index;
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
                    let ident_len = |index: usize| { #ident_len };
                    #(#metadata_arms)*
                    meta.max_depth += #level;
                    meta
                }

                fn traverse_by_key<K, F, E>(mut keys: K, mut func: F) -> Result<usize, ::miniconf::Error<E>>
                where
                    K: ::miniconf::Keys,
                    F: FnMut(usize, Option<&'static str>, usize) -> Result<(), E>,
                {
                    let index = #index;
                    #traverse
                    #increment(match index {
                        #(#traverse_arms ,)*
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

    pub fn tree_serialize(&self) -> TokenStream {
        let depth = self.depth();
        let ident = &self.ident;
        let generics = self.bound_generics(&mut |depth| {
            Some(if depth > 0 {
                parse_quote!(::miniconf::TreeSerialize<#depth>)
            } else {
                parse_quote!(::miniconf::Serialize)
            })
        });

        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let (mat, arms, default): (_, Vec<_>, _) = match &self.data {
            Data::Struct(fields) => (
                quote!(index),
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let rhs = f.serialize_by_key(Some(i));
                        quote!(#i => #rhs)
                    })
                    .collect(),
                quote!(unreachable!()),
            ),
            Data::Enum(variants) => (
                quote!((self, index)),
                variants
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let ident = &v.ident;
                        let rhs = v.field().serialize_by_key(None);
                        quote!((Self::#ident(value, ..), #i) => #rhs)
                    })
                    .collect(),
                quote!(Err(::miniconf::Traversal::Absent(0).into())),
            ),
        };

        let increment = if self.flatten.is_present() {
            quote!()
        } else {
            quote!(::miniconf::Error::increment_result)
        };

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
                    #increment(match #mat {
                        #(#arms ,)*
                        _ => #default
                    })
                }
            }
        }
    }

    pub fn tree_deserialize(&self) -> TokenStream {
        let mut generics = self.bound_generics(&mut |depth| {
            Some(if depth > 0 {
                parse_quote!(::miniconf::TreeDeserialize<'de, #depth>)
            } else {
                parse_quote!(::miniconf::Deserialize<'de>)
            })
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

        let (mat, arms, default): (_, Vec<_>, _) = match &self.data {
            Data::Struct(fields) => (
                quote!(index),
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let rhs = f.deserialize_by_key(Some(i));
                        quote!(#i => #rhs)
                    })
                    .collect(),
                quote!(unreachable!()),
            ),
            Data::Enum(variants) => (
                quote!((self, index)),
                variants
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let ident = &v.ident;
                        let rhs = v.field().deserialize_by_key(None);
                        quote!((Self::#ident(value, ..), #i) => #rhs)
                    })
                    .collect(),
                quote!(Err(::miniconf::Traversal::Absent(0).into())),
            ),
        };

        let increment = if self.flatten.is_present() {
            quote!()
        } else {
            quote!(::miniconf::Error::increment_result)
        };

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
                    #increment(match #mat {
                        #(#arms ,)*
                        _ => #default
                    })
                }
            }
        }
    }

    pub fn tree_any(&self) -> TokenStream {
        let generics = self.bound_generics(&mut |depth| {
            Some(if depth > 0 {
                parse_quote!(::miniconf::TreeAny<#depth>)
            } else {
                parse_quote!(::core::any::Any)
            })
        });

        let depth = self.depth();
        let ident = &self.ident;
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

        let (mat, (ref_arms, mut_arms), default): (_, (Vec<_>, Vec<_>), _) = match &self.data {
            Data::Struct(fields) => (
                quote!(index),
                fields
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let (ref_rhs, mut_rhs) =
                            (f.ref_any_by_key(Some(i)), f.mut_any_by_key(Some(i)));
                        (quote!(#i => #ref_rhs), quote!(#i => #mut_rhs))
                    })
                    .unzip(),
                quote!(unreachable!()),
            ),
            Data::Enum(variants) => (
                quote!((self, index)),
                variants
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        let ident = &v.ident;
                        let (ref_rhs, mut_rhs) = (
                            v.field().ref_any_by_key(None),
                            v.field().mut_any_by_key(None),
                        );
                        (
                            quote!((Self::#ident(value, ..), #i) => #ref_rhs),
                            quote!((Self::#ident(value, ..), #i) => #mut_rhs),
                        )
                    })
                    .unzip(),
                quote!(Err(::miniconf::Traversal::Absent(0).into())),
            ),
        };

        let increment = if self.flatten.is_present() {
            quote!(ret)
        } else {
            quote!(ret.map_err(::miniconf::Traversal::increment))
        };

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
                        let ret: Result<_, _> = match #mat {
                            #(#ref_arms ,)*
                            _ => #default
                        };
                        #increment
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
                        let ret: Result<_, _> = match #mat {
                            #(#mut_arms ,)*
                            _ => #default
                        };
                        #increment
                    }
                }
            }
        }
    }
}

fn walk_type_params<F>(typ: &syn::Type, func: &mut F, depth: usize, generics: &mut syn::Generics)
where
    F: FnMut(usize) -> Option<syn::TraitBound>,
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
                                type_param.bounds.push(bound.into());
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
