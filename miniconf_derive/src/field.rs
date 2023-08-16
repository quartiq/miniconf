use syn::{parenthesized, LitInt};

pub struct StructField {
    pub field: syn::Field,
    pub depth: usize,
}

impl StructField {
    pub fn extract(fields: &syn::Fields) -> Vec<Self> {
        match fields {
            syn::Fields::Named(syn::FieldsNamed { named, .. }) => named,
            syn::Fields::Unnamed(syn::FieldsUnnamed { unnamed, .. }) => unnamed,
            syn::Fields::Unit => unimplemented!("Unit struct not supported"),
        }
        .iter()
        .cloned()
        .map(Self::new)
        .collect()
    }

    pub fn new(field: syn::Field) -> Self {
        let depth = Self::parse_depth(&field);
        Self { field, depth }
    }

    fn parse_depth(field: &syn::Field) -> usize {
        let mut depth = 0;

        for attr in field.attrs.iter() {
            if attr.path().is_ident("tree") {
                depth = 1;
                if matches!(attr.meta, syn::Meta::Path(_)) {
                    continue;
                }
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("depth") {
                        let content;
                        parenthesized!(content in meta.input);
                        let lit: LitInt = content.parse()?;
                        depth = lit.base10_parse()?;
                        Ok(())
                    } else {
                        Err(meta.error(format!("unrecognized miniconf attribute {:?}", meta.path)))
                    }
                })
                .unwrap();
            }
        }
        depth
    }

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

    pub(crate) fn bound_generics<F>(&self, func: &mut F, generics: &mut syn::Generics)
    where
        F: FnMut(usize) -> Option<syn::TypeParamBound>,
    {
        Self::walk_type_params(&self.field.ty, func, self.depth, generics)
    }
}
