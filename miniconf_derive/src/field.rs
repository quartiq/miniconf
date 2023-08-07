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
            if attr.path().is_ident("miniconf") {
                attr.parse_nested_meta(|meta| {
                    if meta.input.is_empty() {
                        depth = 1;
                        Ok(())
                    } else if meta.path.is_ident("defer") {
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
}
