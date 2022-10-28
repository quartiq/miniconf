
use syn::parse_quote;
use super::{TypeDefinition, attributes::{MiniconfAttribute, AttributeParser}};

pub struct StructField {
    pub field: syn::Field,
    pub deferred: bool,
}

impl StructField {
    pub fn new(field: syn::Field) -> Self {
        let deferred = field.attrs.iter().filter(|attr| attr.path.is_ident("miniconf")).map(|attr| AttributeParser::new(attr.tokens.clone()).parse()).any(|x| x == MiniconfAttribute::Defer);

        Self {
            deferred,
            field,
        }
    }

    pub fn bound_generics(&self, typedef: &mut TypeDefinition) {
        // Check our type. Path-like types may need to be bound.
        let path = match &self.field.ty {
            syn::Type::Path(syn::TypePath { path, ..}) => path,
            _ => return,
        };

        // Generics will have an ident only as the type. Grab it.
        let ident = match path.get_ident() {
            Some(ident) => ident,
            _ => return,
        };

        // If we got here, we may have a generic parameter. Check the ident against all of the
        // generic params on the type.
        for generic in &mut typedef.generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                if type_param.ident == *ident {
                    if self.deferred {
                        type_param.bounds.push(parse_quote!(miniconf::Miniconf));
                    } else {
                        type_param.bounds.push(parse_quote!(miniconf::Serialize));
                        type_param.bounds.push(parse_quote!(miniconf::DeserializeOwned));
                    }
                }
            }
        }
    }
}
