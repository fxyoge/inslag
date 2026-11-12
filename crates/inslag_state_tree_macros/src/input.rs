use std::collections::HashSet;

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Expr, Fields, LitStr, spanned::Spanned};

pub(crate) fn expand(
    input: DeriveInput,
    runtime: syn::Result<TokenStream>,
) -> syn::Result<TokenStream> {
    let runtime = runtime?;
    let mut rename_all = None::<LitStr>;
    let mut allow_unknown_fields = false;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("state_tree"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                if rename_all.is_some() {
                    return Err(meta.error("duplicate `rename_all`"));
                }
                rename_all = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("allow_unknown_fields") {
                if allow_unknown_fields {
                    return Err(meta.error("duplicate `allow_unknown_fields`"));
                }
                allow_unknown_fields = true;
            } else {
                return Err(meta.error("expected `rename_all` or `allow_unknown_fields`"));
            }
            Ok(())
        })?;
    }
    let name = &input.ident;
    let mut generics = input.generics.clone();
    if let Data::Enum(data) = &input.data {
        if allow_unknown_fields {
            return Err(Error::new(
                input.span(),
                "`allow_unknown_fields` requires a record",
            ));
        }
        if data
            .variants
            .iter()
            .any(|variant| !matches!(variant.fields, Fields::Unit))
        {
            let mut arms = Vec::new();
            let mut names = HashSet::new();
            for variant in &data.variants {
                let mut authored = None::<LitStr>;
                for attr in variant
                    .attrs
                    .iter()
                    .filter(|attr| attr.path().is_ident("state_tree"))
                {
                    attr.parse_nested_meta(|meta| {
                        if !meta.path.is_ident("name") {
                            return Err(meta.error("expected `name = \"effect.name\"`"));
                        }
                        if authored.is_some() {
                            return Err(meta.error("duplicate `name`"));
                        }
                        authored = Some(meta.value()?.parse()?);
                        Ok(())
                    })?;
                }
                let authored = authored.ok_or_else(|| {
                    Error::new(variant.span(), "effect variants require an authored `name`")
                })?;
                if authored.value().is_empty() || !names.insert(authored.value()) {
                    return Err(Error::new(variant.span(), "empty or duplicate effect name"));
                }
                let ident = &variant.ident;
                let fields = &variant.fields;
                let generics = &input.generics;
                let mut nested: DeriveInput = syn::parse_quote!(
                    struct __StateTreeVariant;
                );
                nested.generics = generics.clone();
                if let Data::Struct(data) = &mut nested.data {
                    data.fields = fields.clone();
                }
                let implementation = expand(nested.clone(), Ok(runtime.clone()))?;
                if let Data::Struct(data) = &mut nested.data {
                    for field in &mut data.fields {
                        field
                            .attrs
                            .retain(|attr| !attr.path().is_ident("state_tree"));
                    }
                }
                let definition = quote!(#nested);
                let (_, ty_generics, _) = generics.split_for_impl();
                let members = fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        field
                            .ident
                            .as_ref()
                            .map(|ident| quote!(#ident))
                            .unwrap_or_else(|| {
                                let index = syn::Index::from(index);
                                quote!(#index)
                            })
                    })
                    .collect::<Vec<_>>();
                let construct = match fields {
                    Fields::Named(_) => quote!(Self::#ident { #(#members: value.#members),* }),
                    Fields::Unnamed(_) => quote!(Self::#ident(#(value.#members),*)),
                    Fields::Unit => quote!(Self::#ident),
                };
                arms.push(quote!(Some(#authored) => {
                    #definition
                    #implementation
                    let value = <__StateTreeVariant #ty_generics as #runtime::StateTreeInput>::from_state_tree_input(context, arguments)?;
                    Ok(#construct)
                }));
            }
            let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
            return Ok(quote! {
                impl #impl_generics #runtime::StateTreeInput for #name #ty_generics #where_clause {
                    fn from_state_tree_input(context: &#runtime::StateTreeActivityContext<'_>, arguments: &mut #runtime::StateTreeActivityArgs) -> ::core::result::Result<Self, #runtime::StateTreeActivityError> {
                        match context.effect_name() {
                            #(#arms,)*
                            _ => Err(#runtime::StateTreeActivityError::new("effect name does not identify a payload variant")),
                        }
                    }
                }
            });
        }
        let snake_case = match rename_all.as_ref().map(LitStr::value).as_deref() {
            None => false,
            Some("snake_case") => true,
            _ => {
                return Err(Error::new(
                    rename_all.unwrap().span(),
                    "expected `snake_case`",
                ));
            }
        };
        let mut arms = Vec::new();
        let mut names = HashSet::new();
        for variant in &data.variants {
            if !matches!(variant.fields, Fields::Unit) {
                return Err(Error::new(
                    variant.span(),
                    "state-tree value enums must have unit variants",
                ));
            }
            let ident = &variant.ident;
            let mut authored = None::<LitStr>;
            for attr in variant
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("state_tree"))
            {
                attr.parse_nested_meta(|meta| {
                    if !meta.path.is_ident("rename") {
                        return Err(meta.error("expected `rename = \"...\"`"));
                    }
                    if authored.is_some() {
                        return Err(meta.error("duplicate `rename`"));
                    }
                    authored = Some(meta.value()?.parse()?);
                    Ok(())
                })?;
            }
            let authored = authored.map(|name| name.value()).unwrap_or_else(|| {
                let ident = ident.to_string();
                if snake_case { snake(&ident) } else { ident }
            });
            if authored.is_empty() || !names.insert(authored.clone()) {
                return Err(Error::new(
                    variant.span(),
                    "empty or duplicate authored variant name",
                ));
            }
            arms.push(quote!(#authored => Ok(Self::#ident),));
        }
        let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
        return Ok(quote! {
            impl #impl_generics #runtime::FromStateTreeValue for #name #ty_generics #where_clause {
                fn from_state_tree_value(value: #runtime::LuauValue) -> ::core::result::Result<Self, ::std::boxed::Box<str>> {
                    let value = <::std::string::String as #runtime::FromStateTreeValue>::from_state_tree_value(value)?;
                    match value.as_str() {
                        #(#arms)*
                        _ => Err(::std::format!("unknown {} value `{value}`", stringify!(#name)).into()),
                    }
                }
            }
        });
    }
    if rename_all.is_some() {
        return Err(Error::new(
            input.span(),
            "`rename_all` is only supported on enums",
        ));
    }
    let Data::Struct(data) = &input.data else {
        return Err(Error::new(
            input.span(),
            "StateTreeInput requires a struct or unit-variant enum",
        ));
    };
    let mut fields = Vec::new();
    let mut names = HashSet::new();
    let mut needs_context = false;
    for (index, field) in data.fields.iter().enumerate() {
        let mut rename = None::<LitStr>;
        let mut default = None::<Option<Expr>>;
        let mut skip = false;
        let mut owner = false;
        let mut activity_id = false;
        let mut flatten = false;
        let mut owner_value = false;
        let mut constant = None::<Expr>;
        let mut activity = None::<LitStr>;
        for attr in field
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("state_tree"))
        {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    if rename.is_some() {
                        return Err(meta.error("duplicate `rename`"));
                    }
                    rename = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("default") {
                    if default.is_some() {
                        return Err(meta.error("duplicate `default`"));
                    }
                    default = Some(if meta.input.peek(syn::Token![=]) {
                        Some(meta.value()?.parse()?)
                    } else {
                        None
                    });
                } else if meta.path.is_ident("skip") {
                    if skip {
                        return Err(meta.error("duplicate `skip`"));
                    }
                    skip = true;
                } else if meta.path.is_ident("default_owner") {
                    if owner {
                        return Err(meta.error("duplicate `default_owner`"));
                    }
                    owner = true;
                } else if meta.path.is_ident("flatten") {
                    if flatten {
                        return Err(meta.error("duplicate `flatten`"));
                    }
                    flatten = true;
                } else if meta.path.is_ident("owner") {
                    if owner_value {
                        return Err(meta.error("duplicate `owner`"));
                    }
                    owner_value = true;
                } else if meta.path.is_ident("value") {
                    if constant.is_some() {
                        return Err(meta.error("duplicate `value`"));
                    }
                    constant = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("activity") {
                    if activity.is_some() {
                        return Err(meta.error("duplicate `activity`"));
                    }
                    activity = Some(meta.value()?.parse()?);
                } else if meta.path.is_ident("activity_id") {
                    if activity_id {
                        return Err(meta.error("duplicate `activity_id`"));
                    }
                    activity_id = true;
                } else {
                    return Err(
                        meta.error("expected a field name, default, flattening, or context source")
                    );
                }
                Ok(())
            })?;
        }
        if [
            default.is_some() && activity.is_none(),
            skip,
            owner,
            activity_id,
            flatten,
            owner_value,
            constant.is_some(),
            activity.is_some(),
        ]
        .into_iter()
        .filter(|set| *set)
        .count()
            > 1
        {
            return Err(Error::new(
                field.span(),
                "conflicting state-tree field sources",
            ));
        }
        needs_context |= owner
            || activity_id
            || owner_value
            || flatten
            || activity.is_some()
            || constant.is_some();
        if rename.is_some()
            && (skip
                || activity_id
                || owner_value
                || flatten
                || constant.is_some()
                || activity.is_some())
        {
            return Err(Error::new(
                field.span(),
                "injected fields cannot be renamed",
            ));
        }
        let authored = rename.map(|name| name.value()).unwrap_or_else(|| {
            field
                .ident
                .as_ref()
                .map(|name| name.to_string().trim_start_matches("r#").to_owned())
                .unwrap_or_else(|| index.to_string())
        });
        if !skip
            && !activity_id
            && !owner_value
            && !flatten
            && constant.is_none()
            && activity.is_none()
            && (!names.insert(authored.clone()) || authored.is_empty())
        {
            return Err(Error::new(field.span(), "empty or duplicate argument name"));
        }
        let ty = &field.ty;
        let value = if let Some(expr) = constant {
            quote!(#expr)
        } else if owner_value {
            quote!(context.entity())
        } else if flatten {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(#ty: #runtime::StateTreeInput));
            quote!(<#ty as #runtime::StateTreeInput>::from_state_tree_input(context, arguments)?)
        } else if let Some(name) = activity {
            match default {
                Some(Some(expr)) => {
                    quote!(context.activity_token(&arguments.take_or_else::<::std::string::String>(#name, || #expr)?))
                }
                Some(None) => {
                    return Err(Error::new(
                        field.span(),
                        "activity defaults require an activity name",
                    ));
                }
                None => {
                    quote!(context.activity_token(&arguments.take::<::std::string::String>(#name)?))
                }
            }
        } else if skip {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(#ty: ::core::default::Default));
            quote!(::core::default::Default::default())
        } else if activity_id {
            quote!(context.activity_id().ok_or_else(|| #runtime::StateTreeActivityError::new("activity input requires an activity ID"))?.into())
        } else {
            generics
                .make_where_clause()
                .predicates
                .push(syn::parse_quote!(#ty: #runtime::FromStateTreeValue));
            if owner {
                quote!(arguments.take_or_else::<#ty>(#authored, || context.entity())?)
            } else if let Some(default) = default {
                let value = if let Some(expr) = default {
                    quote!(#expr)
                } else {
                    generics
                        .make_where_clause()
                        .predicates
                        .push(syn::parse_quote!(#ty: ::core::default::Default));
                    quote!(::core::default::Default::default())
                };
                quote!(arguments.take_or_else::<#ty>(#authored, || #value)?)
            } else {
                quote!(arguments.take::<#ty>(#authored)?)
            }
        };
        fields.push(if let Some(ident) = &field.ident {
            quote!(#ident: #value)
        } else {
            value
        });
    }
    let construct = match &data.fields {
        Fields::Named(_) => quote!(Self { #(#fields),* }),
        Fields::Unnamed(_) => quote!(Self(#(#fields),*)),
        Fields::Unit => quote!(Self),
    };
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let nested = if needs_context {
        quote! {}
    } else {
        let finish = (!allow_unknown_fields).then(|| quote!(arguments.finish()?;));
        quote! {
            impl #impl_generics #runtime::FromStateTreeValue for #name #ty_generics #where_clause {
                fn from_state_tree_value(value: #runtime::LuauValue) -> ::core::result::Result<Self, ::std::boxed::Box<str>> {
                    let values = match value {
                        #runtime::LuauValue::Record(values) => values,
                        #runtime::LuauValue::Sequence(values) if values.is_empty() => ::std::collections::BTreeMap::new(),
                        _ => return Err("expected a record".into()),
                    };
                    let mut arguments = #runtime::StateTreeArgs::from_record(values);
                    (|| -> ::core::result::Result<Self, #runtime::StateTreeActivityError> {
                        let input = #construct;
                        #finish
                        Ok(input)
                    })().map_err(|error| match error.argument_name() {
                        Some(name) => ::std::format!("{name}: {error}").into(),
                        None => error.to_string().into(),
                    })
                }
            }
        }
    };
    Ok(quote! {
        #nested
        impl #impl_generics #runtime::StateTreeInput for #name #ty_generics #where_clause {
            fn from_state_tree_input(
                context: &#runtime::StateTreeActivityContext<'_>,
                arguments: &mut #runtime::StateTreeActivityArgs,
            ) -> ::core::result::Result<Self, #runtime::StateTreeActivityError> {
                Ok(#construct)
            }
        }
    })
}

fn snake(value: &str) -> String {
    let chars: Vec<_> = value.chars().collect();
    let mut result = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_uppercase()
            && index > 0
            && (chars[index - 1].is_lowercase()
                || chars[index - 1].is_numeric()
                || chars.get(index + 1).is_some_and(|next| next.is_lowercase()))
        {
            result.push('_');
        }
        result.extend(ch.to_lowercase());
    }
    result
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use super::{expand, snake};

    #[test]
    fn rejects_conflicting_sources_and_duplicate_names() {
        for input in [
            parse_quote! { struct Input { #[state_tree(default, default_owner)] target: Entity } },
            parse_quote! { struct Input { #[state_tree(skip, activity_id)] id: String } },
            parse_quote! { struct Input { #[state_tree(rename = "a")] first: u32, #[state_tree(rename = "a")] second: u32 } },
            parse_quote! { struct Input { #[state_tree(rename = "")] value: u32 } },
            parse_quote! { struct Input { #[state_tree(rename = "id", activity_id)] value: String } },
            parse_quote! { struct Input { #[state_tree(default, default)] value: u32 } },
        ] {
            assert!(expand(input, Ok(quote!(::runtime))).is_err());
        }
    }

    #[test]
    fn rejects_unsupported_container_and_variant_options() {
        for input in [
            parse_quote! { #[state_tree(try_from = A, try_from = B)] struct Input; },
            parse_quote! { #[state_tree(rename_all = "snake_case")] struct Input; },
            parse_quote! { #[state_tree(rename_all = "camelCase")] enum Input { One } },
            parse_quote! { enum Input { One(u32) } },
            parse_quote! { enum Input { #[state_tree(rename = "one")] One, #[state_tree(rename = "one")] Two } },
            parse_quote! { enum Input { #[state_tree(rename = "")] One } },
            parse_quote! { union Input { one: u32 } },
            parse_quote! { struct Input { #[state_tree(parse = parser)] value: u32 } },
        ] {
            assert!(expand(input, Ok(quote!(::runtime))).is_err());
        }
    }

    #[test]
    fn generic_inputs_preserve_where_clauses_and_use_the_resolved_runtime() {
        let output = expand(
            parse_quote! {
                struct Input<T> where T: Send {
                    value: T,
                    #[state_tree(skip)]
                    marker: std::marker::PhantomData<fn() -> T>,
                }
            },
            Ok(quote!(::renamed_runtime)),
        )
        .unwrap();
        syn::parse2::<syn::File>(output.clone()).unwrap();
        let output = output.to_string();
        assert!(output.contains("renamed_runtime :: StateTreeInput"));
        assert!(output.contains("renamed_runtime :: FromStateTreeValue"));
        assert!(output.contains("T : Send"));
        assert!(!output.contains("T : Default"));
    }

    #[test]
    fn snake_case_handles_acronyms_and_word_boundaries() {
        assert_eq!(snake("WorldSpace"), "world_space");
        assert_eq!(snake("HTTPServer"), "http_server");
        assert_eq!(snake("UI"), "ui");
    }
}
