use syn::{parenthesized, parse_quote, Generics, LitInt};

pub struct StructField {
    pub field: syn::Field,
    pub defer: usize,
}

impl StructField {
    pub fn new(field: syn::Field) -> Self {
        let mut defer = 0;

        for attr in field.attrs.iter() {
            if attr.path().is_ident("miniconf") {
                attr.parse_nested_meta(|meta| {
                    if meta.input.is_empty() {
                        defer = 1;
                        Ok(())
                    } else if meta.path.is_ident("defer") {
                        let content;
                        parenthesized!(content in meta.input);
                        let lit: LitInt = content.parse()?;
                        defer = lit.base10_parse()?;
                        Ok(())
                    } else {
                        Err(meta.error(format!("unrecognized miniconf attribute {:?}", meta.path)))
                    }
                })
                .unwrap();
            }
        }
        Self { defer, field }
    }

    /// Find `ident` in generic parameters and bound it appropriately
    fn bound_type(&self, ident: &syn::Ident, generics: &mut Generics, level: usize) {
        for generic in &mut generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                if type_param.ident == *ident {
                    let depth = self.defer.saturating_sub(level);
                    if depth > 0 {
                        type_param
                            .bounds
                            .push(parse_quote!(::miniconf::Tree<#depth>));
                    } else {
                        type_param.bounds.push(parse_quote!(::miniconf::Serialize));
                        type_param
                            .bounds
                            .push(parse_quote!(::miniconf::DeserializeOwned));
                    }
                }
            }
        }
    }

    /// Handle an individual type encountered in a type definition.
    ///
    /// # Note
    /// This function will recursively travel through arrays/slices,
    /// references, and generics.
    ///
    /// # Args
    /// * `typ` The Type encountered.
    /// * `generics` - The generic type parameters of the structure.
    /// * `level` - The type hierarchy level.
    fn walk_type(&self, typ: &syn::Type, generics: &mut Generics, level: usize) {
        match typ {
            syn::Type::Path(syn::TypePath { path, .. }) => {
                if let Some(ident) = path.get_ident() {
                    // The type is a single ident (no other path segments):
                    // add bounds if it is a generic type for us
                    self.bound_type(ident, generics, level);
                } else {
                    // Analyze the type parameters of the type, as they may be generics for us as well
                    // This tries to reproduce the bounds that field types place on
                    // their generic types, directly or indirectly.
                    //
                    // Assume that all types use their generic T at
                    // relative depth 1, i.e.
                    // * if `#[miniconf(defer(Y > 1))] a: S<T>` then `T: Miniconf<Y - 1>`
                    // * else (i.e. if `Y = 1` or `a: S<T>` without `#[miniconf]`) then `T: SerDe`
                    //
                    // Thus the bounds are conservative (might not be required) and
                    // fragile (might apply the wrong bound).
                    // This matches the standard derive behavior and its issues
                    // https://github.com/rust-lang/rust/issues/26925
                    //
                    // To fix this, one would extend the attribute syntax to allow overriding bounds.
                    for seg in path.segments.iter() {
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            for arg in args.args.iter() {
                                if let syn::GenericArgument::Type(typ) = arg {
                                    // Found type argument in field type: bound it if also in our generics.
                                    self.walk_type(typ, generics, level + 1);
                                }
                            }
                        }
                    }
                }
            }
            syn::Type::Array(syn::TypeArray { elem, .. })
            | syn::Type::Slice(syn::TypeSlice { elem, .. }) => {
                // An array or slice places the element exactly one level deeper: recurse.
                self.walk_type(elem, generics, level + 1);
            }
            syn::Type::Reference(syn::TypeReference { elem, .. }) => {
                // A reference is transparent
                self.walk_type(elem, generics, level);
            }
            other => panic!("Unsupported type: {:?}", other),
        };
    }

    /// Bound the generic parameters of the field.
    ///
    /// # Args
    /// * `generics` The generics for the structure.
    pub(crate) fn bound_generics(&self, generics: &mut Generics) {
        self.walk_type(&self.field.ty, generics, 0)
    }
}
