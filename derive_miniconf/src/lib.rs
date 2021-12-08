use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, DeriveInput};

/// Represents a type definition with associated generics.
struct TypeDefinition {
    pub generics: syn::Generics,
    pub name: syn::Ident,
}

impl TypeDefinition {
    pub fn new(generics: syn::Generics, name: syn::Ident) -> Self {
        let mut typedef = TypeDefinition { generics, name };
        typedef.bound_generics();

        typedef
    }

    /// Bound the generated type definition to only implement when `Self: DeserializeOwned` for
    /// cases when deserialization is required.
    ///
    /// # Note
    /// This is equivalent to adding:
    /// `where Self: DeserializeOwned` to the type definition.
    pub fn add_serde_bound(&mut self) {
        let where_clause = self.generics.make_where_clause();
        where_clause
            .predicates
            .push(parse_quote!(Self: miniconf::DeserializeOwned));
        where_clause
            .predicates
            .push(parse_quote!(Self: miniconf::Serialize));
    }

    // Bound all generics of the type with `T: miniconf::DeserializeOwned + Miniconf`. This is necessary to
    // make `MiniconfAtomic` and enum derives work properly.
    fn bound_generics(&mut self) {
        for generic in &mut self.generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                type_param
                    .bounds
                    .push(parse_quote!(miniconf::DeserializeOwned));
                type_param.bounds.push(parse_quote!(miniconf::Serialize));
                type_param.bounds.push(parse_quote!(miniconf::Miniconf));
            }
        }
    }
}

/// Derive the Miniconf trait for custom types.
///
/// Each field of the struct will be recursively used to construct a unique path for all elements.
///
/// All settings paths are similar to file-system paths with variable names separated by forward
/// slashes.
///
/// For arrays, the array index is treated as a unique identifier. That is, to access the first
/// element of array `test`, the path would be `test/0`.
///
/// # Example
/// ```rust
/// #[derive(Miniconf)]
/// struct Nested {
///     data: [u32; 2],
/// }
/// #[derive(Miniconf)]
/// struct Settings {
///     // Accessed with path `nested/data/0` or `nested/data/1`
///     nested: Nested,
///
///     // Accessed with path `external`
///     external: bool,
/// }
#[proc_macro_derive(Miniconf)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let typedef = TypeDefinition::new(input.generics, input.ident);

    match input.data {
        syn::Data::Struct(struct_data) => derive_struct(typedef, struct_data, false),
        syn::Data::Enum(enum_data) => derive_enum(typedef, enum_data),
        syn::Data::Union(_) => unimplemented!(),
    }
}

/// Derive the Miniconf trait for a custom type that must be updated atomically.
///
/// This derive function should be used if the setting must be updated entirely at once (e.g.
/// individual portions of the struct may not be updated independently).
///
/// See [Miniconf](derive.Miniconf.html) for more information.
///
/// # Example
/// ```rust
/// #[derive(MiniconfAtomic)]
/// struct FilterParameters {
///     coefficient: f32,
///     length: usize,
/// }
///
/// #[derive(Miniconf)]
/// struct Settings {
///     // Accessed with path `filter`, but `filter/length` and `filter/coefficients` are
///     inaccessible.
///     filter: FilterParameters,
///
///     // Accessed with path `external`
///     external: bool,
/// }
#[proc_macro_derive(MiniconfAtomic)]
pub fn derive_atomic(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let typedef = TypeDefinition::new(input.generics, input.ident);

    match input.data {
        syn::Data::Struct(struct_data) => derive_struct(typedef, struct_data, true),
        syn::Data::Enum(enum_data) => derive_enum(typedef, enum_data),
        syn::Data::Union(_) => unimplemented!(),
    }
}

/// Derive the Miniconf trait for structs.
///
/// # Args
/// * `typedef` - The type definition.
/// * `data` - The data associated with the struct definition.
/// * `atomic` - specified true if the data must be updated atomically. If false, data must be
///   set at a terminal node.
///
/// # Returns
/// A token stream of the generated code.
fn derive_struct(mut typedef: TypeDefinition, data: syn::DataStruct, atomic: bool) -> TokenStream {
    let fields = match data.fields {
        syn::Fields::Named(syn::FieldsNamed { ref named, .. }) => named,
        _ => unimplemented!("Only named fields are supported in structs."),
    };

    // If this structure must be updated atomically, it is not valid to call Miniconf recursively
    // on its members.
    if atomic {
        // Bound the Miniconf implementation on Self implementing DeserializeOwned + Serialize.
        typedef.add_serde_bound();

        let name = typedef.name;
        let (impl_generics, ty_generics, where_clause) = typedef.generics.split_for_impl();

        let data = quote! {
            impl #impl_generics miniconf::Miniconf for #name #ty_generics #where_clause {
                fn string_set(&mut self, mut topic_parts:
                core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
                Result<(), miniconf::Error> {
                    if topic_parts.peek().is_some() {
                        return Err(miniconf::Error::AtomicUpdateRequired);
                    }

                    *self = miniconf::serde_json_core::from_slice(value)?.0;
                    Ok(())
                }

                fn string_get(&self, mut topic_parts: core::iter::Peekable<core::str::Split<char>>, value: &mut [u8]) -> Result<usize, miniconf::Error> {
                    if topic_parts.peek().is_some() {
                        return Err(miniconf::Error::AtomicUpdateRequired);
                    }

                    miniconf::serde_json_core::to_slice(self, value).map_err(|_| miniconf::Error::SerializationFailed)
                }

                fn recursive_iter<const TS: usize>(&self, index: &mut [usize], topic: &mut miniconf::heapless::String<TS>) -> Option<()> {
                    if index.len() == 0 {
                        // I don't expect this to happen...
                        panic!("index stack too small");
                        // return None;
                    }

                    let i = index[0];
                    index[0] += 1;

                    if i == 0 {
                        Some(())
                    } else {
                        None
                    }
                }
            }
        };

        return TokenStream::from(data);
    }

    let set_recurse_match_arms = fields.iter().map(|f| {
        let match_name = &f.ident;
        quote! {
            stringify!(#match_name) => {
                self.#match_name.string_set(topic_parts, value)
            }
        }
    });

    let get_recurse_match_arms = fields.iter().map(|f| {
        let match_name = &f.ident;
        quote! {
            stringify!(#match_name) => {
                self.#match_name.string_get(topic_parts, value)
            }
        }
    });

    let iter_match_arms = fields.iter().enumerate().map(|(i, f)| {
        let field_name = &f.ident;
        quote! {
            #i => {
                let postfix = if topic.len() != 0 {
                    concat!("/", stringify!(#field_name))
                } else {
                    stringify!(#field_name)
                };

                if topic.push_str(postfix).is_err() {
                    return None;
                }

                if self.#field_name.recursive_iter(&mut index[1..], topic).is_some() {
                    return Some(());
                }

                // Strip off the previously prepended index, since we completed that element and need
                // to instead check the next one.
                while let Some(character) = topic.pop() {
                    if character == '/' {
                        break;
                    }
                }

                index[0] += 1;
                index[1..].iter_mut().for_each(|x| *x = 0);
            }
        }
    });

    let (impl_generics, ty_generics, where_clause) = typedef.generics.split_for_impl();
    let name = typedef.name;

    let expanded = quote! {
        impl #impl_generics miniconf::Miniconf for #name #ty_generics #where_clause {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                let field = topic_parts.next().ok_or(miniconf::Error::PathTooShort)?;

                match field {
                    #(#set_recurse_match_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn string_get(&self, mut topic_parts: core::iter::Peekable<core::str::Split<char>>, value: &mut [u8]) -> Result<usize, miniconf::Error> {
                let field = topic_parts.next().ok_or(miniconf::Error::PathTooShort)?;

                match field {
                    #(#get_recurse_match_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
                }
            }

            fn recursive_iter<const TS: usize>(&self, index: &mut [usize], topic: &mut miniconf::heapless::String<TS>) -> Option<()> {
                if index.len() == 0 {
                    panic!("index stack too small");
                }

                loop {
                    match index[0] {
                        #(#iter_match_arms ,)*
                        _ => return None,

                    };
                }
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive the Miniconf trait for simple enums.
///
/// # Args
/// * `typedef` - The type definition.
/// * `data` - The data associated with the enum definition.
///
/// # Returns
/// A token stream of the generated code.
fn derive_enum(mut typedef: TypeDefinition, data: syn::DataEnum) -> TokenStream {
    // Only support simple enums, check each field
    for v in data.variants.iter() {
        match v.fields {
            syn::Fields::Named(_) | syn::Fields::Unnamed(_) => {
                unimplemented!("Only simple, C-like enums are supported.")
            }
            syn::Fields::Unit => {}
        }
    }

    typedef.add_serde_bound();

    let (impl_generics, ty_generics, where_clause) = typedef.generics.split_for_impl();
    let name = typedef.name;

    let expanded = quote! {
        impl #impl_generics miniconf::Miniconf for #name #ty_generics #where_clause {
            fn string_set(&mut self, mut topic_parts:
            core::iter::Peekable<core::str::Split<char>>, value: &[u8]) ->
            Result<(), miniconf::Error> {
                if topic_parts.peek().is_some() {
                    // We don't support enums that can contain other values
                    return Err(miniconf::Error::PathTooLong)
                }

                *self = miniconf::serde_json_core::from_slice(value)?.0;
                Ok(())
            }

            fn string_get(&self, mut topic_parts: core::iter::Peekable<core::str::Split<char>>, value: &mut [u8]) -> Result<usize, miniconf::Error> {
                if topic_parts.peek().is_some() {
                    // We don't support enums that can contain other values
                    return Err(miniconf::Error::PathTooLong)
                }

                miniconf::serde_json_core::to_slice(self, value).map_err(|_| miniconf::Error::SerializationFailed)
            }

            fn recursive_iter<const TS: usize>(&self, index: &mut [usize], topic: &mut miniconf::heapless::String<TS>) -> Option<()> {
                if index.len() == 0 {
                    // I don't expect this to happen...
                    panic!("index stack too small");
                    // return None;
                }

                let i = index[0];
                index[0] += 1;

                if i == 0 {
                    Some(())
                } else {
                    None
                }
            }
        }
    };

    TokenStream::from(expanded)
}
