use darling::{util::Flag, FromField};
use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::spanned::Spanned;

#[derive(Debug, FromField, Clone)]
#[darling(attributes(tree))]
pub struct TreeField {
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,
    #[darling(default)]
    pub depth: usize,
    pub skip: Flag,
    pub typ: Option<syn::Type>,
    pub validate: Option<syn::Path>,
    pub get: Option<syn::Path>,
    pub get_mut: Option<syn::Path>,
    pub rename: Option<syn::Ident>,
}

impl TreeField {
    fn span(&self) -> Span {
        self.ident
            .as_ref()
            .map(|i| i.span())
            .unwrap_or(self.ty.span())
    }

    pub fn typ(&self) -> &syn::Type {
        self.typ.as_ref().unwrap_or(&self.ty)
    }

    pub fn name(&self) -> Option<&syn::Ident> {
        self.rename.as_ref().or(self.ident.as_ref())
    }

    fn ident_or_index(&self, i: usize) -> TokenStream {
        match &self.ident {
            None => {
                let index = syn::Index::from(i);
                quote_spanned!(self.span()=> #index)
            }
            Some(name) => quote_spanned!(self.span()=> #name),
        }
    }

    pub fn traverse_by_key(&self, i: usize) -> Option<TokenStream> {
        // Quote context is a match of the field index with `traverse_by_key()` args available.
        let depth = self.depth;
        if depth > 0 {
            let typ = self.typ();
            Some(quote_spanned! { self.span()=>
                #i => <#typ as ::miniconf::TreeKey<#depth>>::traverse_by_key(keys, func)
            })
        } else {
            None
        }
    }

    pub fn metadata(&self, i: usize) -> TokenStream {
        // Quote context is a match of the field index with `metadata()` args available.
        let depth = self.depth;
        if depth > 0 {
            let typ = self.typ();
            quote_spanned! { self.span()=>
                let m = <#typ as ::miniconf::TreeKey<#depth>>::metadata();
                meta.max_length = meta.max_length.max(ident_len(#i) + m.max_length);
                meta.max_depth = meta.max_depth.max(m.max_depth);
                meta.count += m.count;
            }
        } else {
            quote_spanned! { self.span()=>
                meta.max_length = meta.max_length.max(ident_len(#i));
                meta.count += 1;
            }
        }
    }

    fn getter(&self, i: Option<usize>) -> TokenStream {
        if let Some(get) = &self.get {
            quote_spanned! { get.span()=>
                #get(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            }
        } else if let Some(i) = i {
            let ident = self.ident_or_index(i);
            quote_spanned!(self.span()=> ::core::result::Result::Ok(&self.#ident))
        } else {
            quote_spanned!(self.span()=> ::core::result::Result::Ok(value))
        }
    }

    fn getter_mut(&self, i: Option<usize>) -> TokenStream {
        if let Some(get_mut) = &self.get_mut {
            quote_spanned! { get_mut.span()=>
                #get_mut(self).map_err(|msg| ::miniconf::Traversal::Access(0, msg).into())
            }
        } else if let Some(i) = i {
            let ident = self.ident_or_index(i);
            quote_spanned!(self.span()=> ::core::result::Result::Ok(&mut self.#ident))
        } else {
            quote_spanned!(self.span()=> ::core::result::Result::Ok(value))
        }
    }

    fn validator(&self) -> TokenStream {
        if let Some(validate) = &self.validate {
            quote_spanned! { validate.span()=>
                .and_then(|value| #validate(self, value)
                    .map_err(|msg| ::miniconf::Traversal::Invalid(0, msg).into())
                )
            }
        } else {
            quote_spanned!(self.span()=> )
        }
    }

    pub fn serialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `serialize_by_key()` args available.
        let depth = self.depth;
        let getter = self.getter(i);
        if depth > 0 {
            quote_spanned! { self.span()=>
                #getter
                    .and_then(|value|
                        ::miniconf::TreeSerialize::<#depth>::serialize_by_key(value, keys, ser))
            }
        } else {
            quote_spanned! { self.span()=>
                #getter
                    .and_then(|value|
                        ::miniconf::Serialize::serialize(value, ser)
                        .map_err(|err| ::miniconf::Error::Inner(0, err))
                        .and(::core::result::Result::Ok(0))
                    )
            }
        }
    }

    pub fn deserialize_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `deserialize_by_key()` args available.
        let depth = self.depth;
        let getter_mut = self.getter_mut(i);
        let validator = self.validator();
        if depth > 0 {
            quote_spanned! { self.span()=>
                #getter_mut
                    .and_then(|item|
                        ::miniconf::TreeDeserialize::<'de, #depth>::deserialize_by_key(item, keys, de)
                    )
                    #validator
            }
        } else {
            quote_spanned! { self.span()=>
                ::miniconf::Deserialize::deserialize(de)
                    .map_err(|err| ::miniconf::Error::Inner(0, err))
                    #validator
                    .and_then(|new|
                        #getter_mut.map(|item| {
                            *item = new;
                            0
                        })
                    )
            }
        }
    }

    pub fn ref_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.depth;
        let getter = self.getter(i);
        if depth > 0 {
            quote_spanned! { self.span()=>
                #getter
                    .and_then(|item| ::miniconf::TreeAny::<#depth>::ref_any_by_key(item, keys))
            }
        } else {
            quote_spanned! { self.span()=>
                #getter.map(|item| item as &dyn ::core::any::Any)
            }
        }
    }

    pub fn mut_any_by_key(&self, i: Option<usize>) -> TokenStream {
        // Quote context is a match of the field index with `get_mut_by_key()` args available.
        let depth = self.depth;
        let getter_mut = self.getter_mut(i);
        if depth > 0 {
            quote_spanned! { self.span()=>
                #getter_mut
                    .and_then(|item| ::miniconf::TreeAny::<#depth>::mut_any_by_key(item, keys))
            }
        } else {
            quote_spanned! { self.span()=>
                #getter_mut.map(|item| item as &mut dyn ::core::any::Any)
            }
        }
    }
}
