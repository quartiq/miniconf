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
                        return Ok(());
                    }
                    if meta.path.is_ident("defer") {
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

        Self { defer, field }
    }

    fn bound_type(&self, ident: &syn::Ident, generics: &mut Generics, array: bool) {
        for generic in &mut generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                if type_param.ident == *ident {
                    // Deferred array types are a special case. These types defer directly into a
                    // manual implementation of Miniconf that calls serde functions directly.
                    if self.defer.is_some() && !array {
                        // For deferred, non-array data types, we will recursively call into
                        // Miniconf trait functions.
                        let d = self.defer.unwrap();
                        type_param.bounds.push(parse_quote!(miniconf::Miniconf<#d>));
                    } else {
                        // For other data types, we will call into serde functions directly.
                        type_param.bounds.push(parse_quote!(miniconf::Serialize));
                        type_param
                            .bounds
                            .push(parse_quote!(miniconf::DeserializeOwned));
                    }
                }
            }
        }
    }

    /// Handle an individual type encountered in the field type definition.
    ///
    /// # Note
    /// This function will recursively travel through arrays.
    ///
    /// # Note
    /// Only arrays and simple types are currently implemented for type bounds.
    ///
    /// # Args
    /// * `typ` The Type encountered.
    /// * `generics` - The generic type parameters of the structure.
    /// * `array` - Specified true if this type belongs to an upper-level array type.
    fn handle_type(&self, typ: &syn::Type, generics: &mut Generics, array: bool) {
        // Check our type. Path-like types may need to be bound.
        let path = match &typ {
            syn::Type::Path(syn::TypePath { path, .. }) => path,
            syn::Type::Array(syn::TypeArray { elem, .. }) => {
                self.handle_type(elem, generics, true);
                return;
            }
            other => panic!("Unsupported type: {:?}", other),
        };

        // Generics will have an ident only as the type. Grab it.
        if let Some(ident) = path.get_ident() {
            self.bound_type(ident, generics, array);
        }

        // Search for generics in the type signature.
        for segment in path.segments.iter() {
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                for arg in args.args.iter() {
                    if let syn::GenericArgument::Type(typ) = arg {
                        self.handle_type(typ, generics, array);
                    }
                }
            }
        }
    }

    /// Bound the generic parameters of the field.
    ///
    /// # Args
    /// * `generics` The generics for the structure.
    pub(crate) fn bound_generics(&self, generics: &mut Generics) {
        self.handle_type(&self.field.ty, generics, false)
    }
}
