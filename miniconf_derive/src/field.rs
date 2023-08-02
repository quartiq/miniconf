use syn::{parenthesized, parse_quote, Generics, LitInt};

pub struct StructField {
    pub field: syn::Field,
    pub defer: Option<usize>,
}

impl StructField {
    pub fn new(field: syn::Field) -> Self {
        let mut defer = None;

        for attr in field.attrs.iter() {
            if attr.path().is_ident("miniconf") {
                defer = Some(1);
                attr.parse_nested_meta(|meta| {
                    if meta.input.is_empty() {
                        Ok(())
                    } else if meta.path.is_ident("defer") {
                        let content;
                        parenthesized!(content in meta.input);
                        let lit: LitInt = content.parse()?;
                        defer = Some(lit.base10_parse()?);
                        Ok(())
                    } else {
                        Err(meta.error(format!("unrecognized miniconf attribute {:?}", meta.path)))
                    }
                })
                .unwrap();
            }
        }
        defer.map(|d| assert!(d > 0));
        Self { defer, field }
    }

    /// Find `ident` in generic parameters and bound it appropriately
    fn bound_type(&self, ident: &syn::Ident, generics: &mut Generics, depth: usize) {
        for generic in &mut generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                if type_param.ident == *ident {
                    if self.defer.unwrap_or_default().saturating_sub(depth) > 0 {
                        type_param
                            .bounds
                            .push(parse_quote!(miniconf::Miniconf<#depth>));
                    } else {
                        type_param.bounds.push(parse_quote!(miniconf::Serialize));
                        type_param
                            .bounds
                            .push(parse_quote!(miniconf::DeserializeOwned));
                    }
                }
            }
        }
    }

    /// Handle an individual type encountered in a type definition.
    ///
    /// # Note
    /// This function will recursively travel through arrays and generics.
    ///
    /// # Args
    /// * `typ` The Type encountered.
    /// * `generics` - The generic type parameters of the structure.
    /// * `depth` - The type hierarchy recursion depth.
    fn handle_type(&self, typ: &syn::Type, generics: &mut Generics, depth: usize) {
        match typ {
            syn::Type::Path(syn::TypePath { path, .. }) => {
                if let Some(ident) = path.get_ident() {
                    // The type is a single ident (no other path segments): bound it if itself is a generic type for us
                    self.bound_type(ident, generics, depth);
                } else {
                    // Analyze the generics of the type, as they may be generics for us as well
                    for seg in path.segments.iter() {
                        // This tries to reproduce the bounds that field types place on
                        // their generic types, directly or indirectly.
                        //
                        // Assume that all types use their generic T at
                        // relative depth 1, i.e.
                        // * if `#[miniconf(defer(Y > 1))] a: S<T>` then `T: Miniconf<Y - 1>`
                        // * else (i.e. if `Y <= 1` or `S<T>` is used without `#[miniconf]`) then `T: SerDe`
                        //
                        // Thus the bounds are conservative (might not be required) and
                        // fragile (might apply the wrong bound).
                        // This matches the standard derive behavior and its issues
                        // https://github.com/rust-lang/rust/issues/26925
                        //
                        // To fix this, one would extend the attribute syntax to allow overriding bounds.
                        if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                            for arg in args.args.iter() {
                                if let syn::GenericArgument::Type(typ) = arg {
                                    // Found type argument in field type: bound it if also in our generics.
                                    self.handle_type(typ, generics, depth + 1);
                                }
                            }
                        }
                    }
                }
            }
            syn::Type::Array(syn::TypeArray { elem, .. })
            | syn::Type::Slice(syn::TypeSlice { elem, .. }) => {
                // An array or slice places the element exactly one level deeper: recurse.
                self.handle_type(elem, generics, depth + 1);
            }
            syn::Type::Reference(syn::TypeReference { elem, .. }) => {
                // A reference is transparent
                self.handle_type(elem, generics, depth);
            }
            other => panic!("Unsupported type: {:?}", other),
        };
    }

    /// Bound the generic parameters of the field.
    ///
    /// # Args
    /// * `generics` The generics for the structure.
    pub(crate) fn bound_generics(&self, generics: &mut Generics) {
        self.handle_type(&self.field.ty, generics, 0)
    }
}
