use darling::{ast, util::Flag, FromDeriveInput, FromField, FromMeta};
use syn::Path;

#[derive(Debug, FromField)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    pub vis: syn::Visibility,
    pub ty: syn::Type,
    // attrs: Vec<syn::Attribute>,
    #[darling(default)]
    pub depth: usize,
    pub skip: Flag,
    pub validate: Option<Path>,
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(tree))]
#[darling(supports(struct_any))]
pub struct Tree {
    pub ident: syn::Ident,
    pub generics: syn::Generics,
    pub vis: syn::Visibility,
    pub data: ast::Data<(), TreeField>,
    // attrs: Vec<syn::Attribute>,
}

impl TreeField {
    fn walk_type_params<F>(
        typ: &syn::Type,
        func: &mut F,
        depth: usize,
        generics: &mut syn::Generics,
    ) where
        F: FnMut(usize) -> Option<syn::TypeParamBound>,
    {
        match typ {
            syn::Type::Path(syn::TypePath { path, .. }) => {
                if let Some(ident) = path.get_ident() {
                    // The type is a single ident (no other path segments, has no generics):
                    // call back if it is a generic type for us
                    for generic in &mut generics.params {
                        if let syn::GenericParam::Type(type_param) = generic {
                            if type_param.ident == *ident {
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
                                    Self::walk_type_params(
                                        typ,
                                        func,
                                        depth.saturating_sub(1),
                                        generics,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            syn::Type::Array(syn::TypeArray { elem, .. })
            | syn::Type::Slice(syn::TypeSlice { elem, .. }) => {
                // An array or slice places the element exactly one level deeper: recurse.
                Self::walk_type_params(elem, func, depth.saturating_sub(1), generics);
            }
            syn::Type::Reference(syn::TypeReference { elem, .. }) => {
                // A reference is transparent
                Self::walk_type_params(elem, func, depth, generics);
            }
            other => panic!("Unsupported type: {:?}", other),
        };
    }

    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn bound_generics<F>(&self, func: &mut F, generics: &mut syn::Generics)
    where
        F: FnMut(usize) -> Option<syn::TypeParamBound>,
    {
        Self::walk_type_params(&self.ty, func, self.depth, generics)
    }
}
