use super::{
    attributes::{AttributeParser, MiniconfAttribute},
    TypeDefinition,
};
use syn::parse_quote;

pub struct StructField {
    pub field: syn::Field,
    pub deferred: bool,
}

impl StructField {
    pub fn new(field: syn::Field) -> Self {
        let attributes: Vec<MiniconfAttribute> = field
            .attrs
            .iter()
            .filter(|attr| attr.path.is_ident("miniconf"))
            .map(|attr| AttributeParser::new(attr.tokens.clone()).parse())
            .collect();

        let atomic = attributes.iter().any(|x| *x == MiniconfAttribute::Atomic);

        // Arrays are deferred-by-default unless explicitly specified otherwise.
        let deferred = attributes.iter().any(|x| *x == MiniconfAttribute::Defer)
            || (matches!(field.ty, syn::Type::Array(_)) && !atomic);

        Self { deferred, field }
    }

    fn bound_type(&self, ident: &syn::Ident, typedef: &mut TypeDefinition, array: bool) {
        for generic in &mut typedef.generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                if type_param.ident == *ident {
                    // Deferred array types are a special case. These types defer directly into a
                    // manual implementation of Miniconf that calls serde functions directly.
                    if self.deferred && !array {
                        // For deferred, non-array data types, we will recursively call into
                        // Miniconf trait functions.
                        type_param.bounds.push(parse_quote!(miniconf::Miniconf));
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
    /// * `typedef` - The generic type parameters of the structure.
    /// * `array` - Specified true if this type belongs to an upper-level array type.
    fn handle_type(&self, typ: &syn::Type, typedef: &mut TypeDefinition, array: bool) {
        // Check our type. Path-like types may need to be bound.
        let path = match &typ {
            syn::Type::Path(syn::TypePath { path, .. }) => path,
            syn::Type::Array(syn::TypeArray { elem, .. }) => {
                self.handle_type(elem, typedef, true);
                return;
            }
            other => panic!("Unsupported type: {:?}", other),
        };

        // Generics will have an ident only as the type. Grab it.
        if let Some(ident) = path.get_ident() {
            self.bound_type(ident, typedef, array);
        }

        // Search for generics in the type signature.
        for segment in path.segments.iter() {
            if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                for arg in args.args.iter() {
                    if let syn::GenericArgument::Type(typ) = arg {
                        self.handle_type(typ, typedef, array);
                    }
                }
            }
        }
    }

    /// Bound the generic parameters of the field.
    ///
    /// # Args
    /// * `typedef` The typedefinitions for the structure.
    pub(crate) fn bound_generics(&self, typedef: &mut TypeDefinition) {
        self.handle_type(&self.field.ty, typedef, false)
    }
}
