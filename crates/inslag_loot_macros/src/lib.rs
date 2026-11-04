//! Derives for typed loot arguments and results, re-exported by `inslag_loot`.

#![warn(missing_docs)]

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, LitStr, Type, parse_macro_input, parse_quote};

fn crate_path() -> proc_macro2::TokenStream {
    let root = match proc_macro_crate::crate_name("inslag_loot") {
        Ok(proc_macro_crate::FoundCrate::Itself) => quote!(::inslag_loot),
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let ident = syn::Ident::new(&name, proc_macro2::Span::call_site());
            quote!(::#ident)
        }
        Err(_) => quote!(::inslag_loot),
    };
    quote!(#root::__private)
}

#[derive(Default)]
struct Attributes {
    primary: bool,
    default: Option<Expr>,
    rename: Option<LitStr>,
    reference: Option<LitStr>,
}

fn attributes(attrs: &[syn::Attribute]) -> syn::Result<Attributes> {
    let mut result = Attributes::default();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("loot")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("primary") && !result.primary {
                result.primary = true;
            } else if meta.path.is_ident("default") && result.default.is_none() {
                result.default = Some(if meta.input.peek(syn::Token![=]) {
                    meta.value()?.parse()?
                } else {
                    parse_quote!(::core::default::Default::default())
                });
            } else if meta.path.is_ident("rename") && result.rename.is_none() {
                result.rename = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("reference") && result.reference.is_none() {
                result.reference = Some(meta.value()?.parse()?);
            } else {
                return Err(meta.error("unknown or duplicate loot attribute"));
            }
            Ok(())
        })?;
    }
    Ok(result)
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(path) = ty else { return None };
    let segment = path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    }
}

/// Exposes struct fields as named loot arguments.
///
/// `#[loot(primary)]` selects the unnamed argument; `#[loot(rename = "name")]`
/// changes a field's loot name. `Option<T>` fields may be omitted, as may fields
/// with `#[loot(default)]` or `#[loot(default = expression)]`.
/// `#[loot(reference = "kind")]` restricts a `Reference` field to that kind.
#[proc_macro_derive(LootArgs, attributes(loot))]
pub fn loot_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    args_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn args_impl(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let root = crate_path();
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(name, "LootArgs requires a struct"));
    };
    let mut signature = Vec::new();
    let mut extraction = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    let mut primary = false;
    let mut generics = input.generics.clone();
    for field in &data.fields {
        let Some(ident) = &field.ident else {
            return Err(syn::Error::new_spanned(
                field,
                "LootArgs requires named fields",
            ));
        };
        let ty = &field.ty;
        let inner = option_inner(ty);
        let value_ty = inner.unwrap_or(ty);
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#value_ty: #root::LootValue + #root::FromLootValue));
        let attrs = attributes(&field.attrs)?;
        let field_name = attrs.rename.unwrap_or_else(|| {
            LitStr::new(ident.to_string().trim_start_matches("r#"), ident.span())
        });
        if !names.insert(field_name.value()) {
            return Err(syn::Error::new_spanned(
                field,
                "duplicate loot argument name",
            ));
        }
        let value_type = if let Some(kind) = attrs.reference {
            if !matches!(value_ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "Reference"))
            {
                return Err(syn::Error::new_spanned(
                    field,
                    "reference requires a Reference field",
                ));
            }
            quote!(#root::ValueType::reference(#kind))
        } else {
            quote!(<#value_ty as #root::LootValue>::value_type())
        };
        if let Some(inner) = inner {
            signature.push(quote!(signature = signature.omittable(#field_name, #value_type);));
            if let Some(default) = attrs.default {
                extraction.push(quote!(#ident: match args.take_optional::<#inner>(#field_name)? { Some(value) => Some(value), None => #default }));
            } else {
                extraction.push(quote!(#ident: args.take_optional::<#inner>(#field_name)?));
            }
        } else if let Some(default) = attrs.default {
            signature.push(quote!(signature = signature.omittable(#field_name, #value_type);));
            extraction.push(
                quote!(#ident: args.take_optional::<#ty>(#field_name)?.unwrap_or_else(|| #default)),
            );
        } else {
            signature.push(quote!(signature = signature.required(#field_name, #value_type);));
            extraction.push(quote!(#ident: args.take::<#ty>(#field_name)?));
        }
        if attrs.primary {
            if primary {
                return Err(syn::Error::new_spanned(
                    field,
                    "only one primary argument is allowed",
                ));
            }
            primary = true;
            signature.push(quote!(signature = signature.primary(#field_name);));
        }
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let construct = if matches!(data.fields, Fields::Unit) {
        quote!(Self)
    } else {
        quote!(Self { #(#extraction),* })
    };
    Ok(quote! {
        impl #impl_generics #root::LootArgs for #name #ty_generics #where_clause {
            fn signature() -> #root::FunctionSignature {
                let mut signature = #root::FunctionSignature::new();
                #(#signature)*
                signature
            }
            fn from_arguments(args: &mut #root::Arguments) -> Result<Self, String> {
                Ok(#construct)
            }
        }
    })
}

/// Makes a type usable as a generator result and a requested loot output.
///
/// Structs are preserved as opaque Rust data. Enums combine variants wrapping
/// distinct output types; `#[loot(opaque)]` instead preserves the entire enum as
/// Rust data. Values matching multiple variants are rejected as ambiguous.
#[proc_macro_derive(LootOutput, attributes(loot))]
pub fn loot_output(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    output_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn output_impl(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let root = crate_path();
    let name = &input.ident;
    let mut opaque = false;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("loot"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("opaque") && !opaque {
                opaque = true;
                Ok(())
            } else {
                Err(meta.error("expected opaque"))
            }
        })?;
    }
    if let Data::Enum(data) = &input.data
        && !opaque
    {
        let mut accepts = Vec::new();
        let mut extraction = Vec::new();
        let mut accepts_value = Vec::new();
        let mut conversions = Vec::new();
        let mut value_types = Vec::new();
        let mut into_value = Vec::new();
        let mut types = std::collections::BTreeSet::new();
        let mut generics = input.generics.clone();
        for variant in &data.variants {
            let Fields::Unnamed(fields) = &variant.fields else {
                return Err(syn::Error::new_spanned(
                    variant,
                    "output variants require one unnamed field",
                ));
            };
            if fields.unnamed.len() != 1 {
                return Err(syn::Error::new_spanned(
                    variant,
                    "output variants require one unnamed field",
                ));
            }
            let ty = &fields.unnamed[0].ty;
            if !types.insert(quote!(#ty).to_string()) {
                return Err(syn::Error::new_spanned(variant, "duplicate output type"));
            }
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #root::LootOutput));
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #root::LootValue));
            let ident = &variant.ident;
            accepts.push(quote!(<#ty as #root::LootOutput>::accepts_type(value_type)));
            accepts_value.push(quote!(<#ty as #root::LootOutput>::accepts_value(value)));
            extraction.push(quote! {
                if <#ty as #root::LootOutput>::accepts_value(&value) {
                    return <#ty as #root::LootOutput>::from_value(value).map(Self::#ident);
                }
            });
            conversions.push((ty, ident));
            value_types.push(quote!(<#ty as #root::LootValue>::value_type()));
            into_value
                .push(quote!(Self::#ident(value) => <#ty as #root::LootValue>::into_value(value)));
        }
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        let from = conversions.iter().map(|(ty, ident)| {
            quote! {
                impl #impl_generics From<#ty> for #name #ty_generics #where_clause {
                    fn from(value: #ty) -> Self { Self::#ident(value) }
                }
            }
        });
        return Ok(quote! {
            impl #impl_generics #root::LootOutput for #name #ty_generics #where_clause {
                fn accepts_type(value_type: &#root::ValueType) -> bool {
                    if let #root::ValueType::OneOf(types) = value_type {
                        return types.iter().all(<Self as #root::LootOutput>::accepts_type);
                    }
                    (0usize #(+ usize::from(#accepts))*) == 1
                }
                fn accepts_value(value: &#root::Value) -> bool { (0usize #(+ usize::from(#accepts_value))*) == 1 }
                fn from_value(value: #root::Value) -> Result<Self, String> {
                    let matches = { let value = &value; 0usize #(+ usize::from(#accepts_value))* };
                    if matches > 1 { return Err(format!("ambiguous output for {}: {:?}", stringify!(#name), value)); }
                    #(#extraction)*
                    Err(format!("unsupported output for {}: {:?}", stringify!(#name), value))
                }
            }
            impl #impl_generics #root::FromLootValue for #name #ty_generics #where_clause {
                fn from_loot_value(value: #root::Value) -> Result<Self, String> { <Self as #root::LootOutput>::from_value(value) }
            }
            impl #impl_generics #root::LootValue for #name #ty_generics #where_clause {
                fn value_type() -> #root::ValueType { #root::ValueType::OneOf(::std::sync::Arc::from([#(#value_types),*])) }
                fn into_value(self) -> #root::Value { match self { #(#into_value),* } }
            }
            impl #impl_generics #root::LootReturn for #name #ty_generics #where_clause {
                fn output_type() -> Option<#root::ValueType> { Some(<Self as #root::LootValue>::value_type()) }
                fn into_values(self) -> Result<Vec<#root::Value>, String> { Ok(vec![<Self as #root::LootValue>::into_value(self)]) }
            }
            #(#from)*
        });
    }
    if !matches!(input.data, Data::Struct(_)) && !opaque {
        return Err(syn::Error::new_spanned(
            name,
            "LootOutput requires a struct or enum",
        ));
    }
    let mut generics = input.generics.clone();
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: Clone + ::core::fmt::Debug + PartialEq + Send + Sync + 'static));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let output = atom_output(&root, name, &generics);
    Ok(quote! {
        impl #impl_generics #root::FromLootValue for #name #ty_generics #where_clause {
            fn from_loot_value(value: #root::Value) -> Result<Self, String> {
                match value {
                    #root::Value::Data(data) => {
                        let value = data.downcast::<Self>().map_err(|_| format!("expected {}", stringify!(#name)))?;
                        Ok(::std::sync::Arc::unwrap_or_clone(value))
                    }
                    _ => Err(format!("expected {}", stringify!(#name))),
                }
            }
        }
        impl #impl_generics #root::LootValue for #name #ty_generics #where_clause {
            fn value_type() -> #root::ValueType { #root::ValueType::data::<Self>() }
            fn into_value(self) -> #root::Value { #root::Value::data(self) }
        }
        #output
    })
}

fn atom_output(
    root: &proc_macro2::TokenStream,
    name: &syn::Ident,
    generics: &syn::Generics,
) -> proc_macro2::TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics #root::LootReturn for #name #ty_generics #where_clause {
            fn output_type() -> Option<#root::ValueType> { Some(<Self as #root::LootValue>::value_type()) }
            fn into_values(self) -> Result<Vec<#root::Value>, String> { Ok(vec![<Self as #root::LootValue>::into_value(self)]) }
        }
        impl #impl_generics #root::LootOutput for #name #ty_generics #where_clause {
            fn accepts_type(value_type: &#root::ValueType) -> bool { <Self as #root::LootValue>::value_type().contains(value_type) }
            fn accepts_value(value: &#root::Value) -> bool { <Self as #root::LootValue>::value_type().accepts(value) }
            fn from_value(value: #root::Value) -> Result<Self, String> { <Self as #root::FromLootValue>::from_loot_value(value) }
        }
    }
}

/// Maps a fieldless enum to strings usable in loot sources.
///
/// Variant names become lowercase with underscores before subsequent uppercase
/// letters. `#[loot(rename = "value")]` overrides a variant's string.
#[proc_macro_derive(LootValue, attributes(loot))]
pub fn loot_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    value_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn value_impl(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let root = crate_path();
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            name,
            "LootValue requires a fieldless enum; use LootOutput for opaque structs",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new_spanned(
            name,
            "LootValue requires at least one variant",
        ));
    }
    let mut names = Vec::new();
    let mut variants = Vec::new();
    let mut unique = std::collections::BTreeSet::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new_spanned(
                variant,
                "LootValue requires fieldless variants",
            ));
        }
        let attrs = attributes(&variant.attrs)?;
        if attrs.primary || attrs.default.is_some() || attrs.reference.is_some() {
            return Err(syn::Error::new_spanned(
                variant,
                "LootValue variants only support rename",
            ));
        }
        let string = variant.ident.to_string();
        let mut snake = String::new();
        for (index, c) in string.chars().enumerate() {
            if index > 0 && c.is_uppercase() {
                snake.push('_');
            }
            snake.extend(c.to_lowercase());
        }
        let label = attrs
            .rename
            .unwrap_or_else(|| LitStr::new(&snake, variant.ident.span()));
        if !unique.insert(label.value()) {
            return Err(syn::Error::new_spanned(variant, "duplicate loot value"));
        }
        names.push(label);
        variants.push(&variant.ident);
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let output = atom_output(&root, name, &input.generics);
    Ok(quote! {
        impl #impl_generics #root::FromLootValue for #name #ty_generics #where_clause {
            fn from_loot_value(value: #root::Value) -> Result<Self, String> {
                match value {
                    #root::Value::String(value) => match value.as_ref() {
                        #(#names => Ok(Self::#variants),)*
                        _ => Err(format!("expected one of {:?}, got {:?}", [#(#names),*], value)),
                    },
                    _ => Err(format!("expected {}", stringify!(#name))),
                }
            }
        }
        impl #impl_generics #root::LootValue for #name #ty_generics #where_clause {
            fn value_type() -> #root::ValueType { #root::ValueType::StringEnum(&[#(#names),*]) }
            fn into_value(self) -> #root::Value { match self { #(Self::#variants => #root::Value::from(#names),)* } }
        }
        #output
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_reject_ambiguous_or_unsupported_declarations() {
        for (input, message) in [
            (
                parse_quote!(
                    enum Args {
                        A,
                    }
                ),
                "requires a struct",
            ),
            (
                parse_quote!(
                    struct Args(String);
                ),
                "named fields",
            ),
            (
                parse_quote!(
                    struct Args {
                        #[loot(primary)]
                        a: String,
                        #[loot(primary)]
                        b: String,
                    }
                ),
                "one primary",
            ),
            (
                parse_quote!(
                    struct Args {
                        a: String,
                        #[loot(rename = "a")]
                        b: String,
                    }
                ),
                "duplicate loot argument",
            ),
            (
                parse_quote!(
                    struct Args {
                        #[loot(primary, primary)]
                        a: String,
                    }
                ),
                "duplicate loot attribute",
            ),
            (
                parse_quote!(
                    struct Args {
                        #[loot(reference = "item")]
                        a: String,
                    }
                ),
                "Reference field",
            ),
        ] {
            assert!(args_impl(input).unwrap_err().to_string().contains(message));
        }
    }

    #[test]
    fn output_enums_require_distinct_single_field_variants() {
        for input in [
            parse_quote!(
                enum Output {
                    Empty,
                }
            ),
            parse_quote!(
                enum Output {
                    Pair(String, i64),
                }
            ),
            parse_quote!(
                enum Output {
                    Named { value: String },
                }
            ),
            parse_quote!(
                enum Output {
                    A(String),
                    B(String),
                }
            ),
        ] {
            assert!(output_impl(input).is_err());
        }
        assert!(
            output_impl(parse_quote!(
                #[loot(opaque)]
                enum Output {
                    Empty,
                    Named { value: String },
                }
            ))
            .is_ok()
        );
    }

    #[test]
    fn value_enums_reject_duplicate_names_and_inapplicable_attributes() {
        for input in [
            parse_quote!(
                enum Kind {}
            ),
            parse_quote!(
                enum Kind {
                    Value(String),
                }
            ),
            parse_quote!(
                enum Kind {
                    A,
                    #[loot(rename = "a")]
                    B,
                }
            ),
            parse_quote!(
                enum Kind {
                    #[loot(default)]
                    A,
                }
            ),
        ] {
            assert!(value_impl(input).is_err());
        }
    }
}
