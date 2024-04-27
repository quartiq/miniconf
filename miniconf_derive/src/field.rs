use darling::{
    ast::{self, Data},
    util::Flag,
    Error, FromDeriveInput, FromField,
};
use proc_macro2::TokenStream;
use quote::quote;

#[derive(Debug, FromField)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    // pub vis: syn::Visibility,
    pub ty: syn::Type,
    // attrs: Vec<syn::Attribute>,
    #[darling(default)]
    pub depth: usize,
    pub skip: Flag,
    pub typ: Option<syn::Type>,
    pub validate: Option<syn::Path>,
    pub get: Option<syn::Path>,
    pub get_mut: Option<syn::Path>,
}

impl TreeField {
    pub(crate) fn typ(&self) -> &syn::Type {
        self.typ.as_ref().unwrap_or(&self.ty)
    }

    pub(crate) fn name_or_index(&self, i: usize) -> TokenStream {
        match &self.ident {
            None => {
                let index = syn::Index::from(i);
                quote! { #index }
            }
            Some(name) => quote! { #name },
        }
    }

    pub(crate) fn traverse_by_key(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `traverse_by_key()` args available.
        let depth = self.depth;
        if depth > 0 {
            let field_type = self.typ();
            Some(quote! {
                #i => <#field_type as ::miniconf::TreeKey<#depth>>::traverse_by_key(keys, func)
            })
        } else {
            None
        }
    }

    pub(crate) fn metadata(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `metadata()` args available.
        let depth = self.depth;
        if depth > 0 {
            let field_type = self.typ();
            Some(quote! {
                #i => <#field_type as ::miniconf::TreeKey<#depth>>::metadata()
            })
        } else {
            None
        }
    }

    fn getter(&self, i: usize) -> TokenStream {
        let ident = self.name_or_index(i);
        match &self.get {
            Some(get) => quote! {
                #get(self).map_err(|msg| ::miniconf::Error::Access(0, msg))
            },
            None => quote! { Ok(&self.#ident) },
        }
    }

    fn getter_mut(&self, i: usize) -> TokenStream {
        let ident = self.name_or_index(i);
        match &self.get_mut {
            Some(get_mut) => quote!(
                #get_mut(self).map_err(|msg| ::miniconf::Error::Access(0, msg))
            ),
            None => quote!( Ok(&mut self.#ident) ),
        }
    }

    fn validator(&self) -> TokenStream {
        match &self.validate {
            Some(validate) => quote! { |value|
                #validate(self, value).map_err(|msg| ::miniconf::Error::Invalid(0, msg))
            },
            None => quote! { |value| Ok(value) },
        }
    }

    pub(crate) fn serialize_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let depth = self.depth;
        let getter = self.getter(i);
        if depth > 0 {
            quote! {
                #i => #getter
                    .and_then(|value|
                        ::miniconf::TreeSerialize::<#depth>::serialize_by_key(value, keys, ser))
            }
        } else {
            quote! {
                #i => #getter
                    .and_then(|value|
                        ::miniconf::Serialize::serialize(value, ser)
                        .map_err(::miniconf::Error::Inner)
                        .and(Ok(0))
                    )
            }
        }
    }

    pub(crate) fn deserialize_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        let depth = self.depth;
        let getter_mut = self.getter_mut(i);
        let validator = self.validator();
        if depth > 0 {
            quote! {
                #i => #getter_mut
                    .and_then(|item|
                        ::miniconf::TreeDeserialize::<'de, #depth>::deserialize_by_key(item, keys, de)
                    )
                    .and_then(#validator)
            }
        } else {
            quote! {
                #i => ::miniconf::Deserialize::deserialize(de)
                    .map_err(::miniconf::Error::Inner)
                    .and_then(#validator)
                    .and_then(|value|
                        #getter_mut.and_then(|item| {
                            *item = value;
                            Ok(0)
                        })
                    )
            }
        }
    }

    pub(crate) fn get_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.depth;
        let getter = self.getter(i);
        if depth > 0 {
            quote! {
                #i => #getter
                    .and_then(|value| ::miniconf::TreeAny::<#depth>::get_by_key(value, keys))
            }
        } else {
            quote! {
                #i => #getter.map(|value| value as &dyn ::core::any::Any)
            }
        }
    }

    pub(crate) fn get_mut_by_key(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.depth;
        let getter_mut = self.getter_mut(i);
        if depth > 0 {
            quote! {
                #i => #getter_mut
                    .and_then(|value| ::miniconf::TreeAny::<#depth>::get_mut_by_key(value, keys))
            }
        } else {
            quote! {
                #i => #getter_mut.map(|value| value as &mut dyn ::core::any::Any)
            }
        }
    }
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(tree))]
#[darling(supports(struct_any))]
pub struct Tree {
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    // pub vis: syn::Visibility,
    pub data: ast::Data<(), TreeField>,
    // attrs: Vec<syn::Attribute>,
}

impl Tree {
    pub(crate) fn depth(&self) -> usize {
        self.fields()
            .iter()
            .fold(0usize, |d, field| d.max(field.depth))
            + 1
    }

    pub(crate) fn parse(input: &syn::DeriveInput) -> Result<Self, Error> {
        let mut t = Self::from_derive_input(input)?;
        t.fields_mut().retain(|f| !f.skip.is_present());
        Ok(t)
    }

    pub(crate) fn fields(&self) -> &Vec<TreeField> {
        let Data::Struct(fields) = &self.data else {
            unreachable!()
        };
        &fields.fields
    }

    pub(crate) fn fields_mut(&mut self) -> &mut Vec<TreeField> {
        let Data::Struct(fields) = &mut self.data else {
            unreachable!()
        };
        &mut fields.fields
    }

    pub(crate) fn bound_generics<F>(&mut self, func: &mut F)
    where
        F: FnMut(usize) -> Option<syn::TypeParamBound>,
    {
        let Self {
            ref mut generics,
            data: Data::Struct(ref fields),
            ..
        } = self
        else {
            unreachable!()
        };
        for f in fields.fields.iter() {
            walk_type_params(f.typ(), func, f.depth, generics)
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
