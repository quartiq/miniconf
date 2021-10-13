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
    pub fn add_deserialize_bound(&mut self) {
        let where_clause = self.generics.make_where_clause();
        where_clause
            .predicates
            .push(parse_quote!(Self: miniconf::DeserializeOwned));
    }

    // Bound all generics of the type with `T: miniconf::DeserializeOwned`. This is necessary to
    // make `MiniconfAtomic` and enum derives work properly.
    fn bound_generics(&mut self) {
        for generic in &mut self.generics.params {
            if let syn::GenericParam::Type(type_param) = generic {
                type_param
                    .bounds
                    .push(parse_quote!(miniconf::DeserializeOwned));
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
        // Bound the Miniconf implementation on Self implementing DeserializeOwned.
        typedef.add_deserialize_bound();

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
            }
        };

        return TokenStream::from(data);
    }

    let recurse_match_arms = fields.iter().map(|f| {
        let match_name = &f.ident;
        quote! {
            stringify!(#match_name) => {
                self.#match_name.string_set(topic_parts, value)?;
                Ok(())
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
                    #(#recurse_match_arms ,)*
                    _ => Err(miniconf::Error::PathNotFound)
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

    typedef.add_deserialize_bound();

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
        }
    };

    TokenStream::from(expanded)
}
