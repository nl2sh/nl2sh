//! Compile-time adapter generation; registration remains explicit.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, punctuated::Punctuated, Expr, ItemFn, Lit, LitStr, MetaNameValue, Token,
};

/// Generates a `Tool` adapter for an async prepare function.
///
/// Syntax: `#[tool(adapter = "AdapterName", name = "tool_name", description = "...",
/// category = "knowledge", risk = "read_only", requires = ["ima"], parallel_safe = true)]`.
/// Optional policies: group, default_enabled, platform, runtime, lifetime and concurrency.
/// Specify either concurrency or the legacy parallel_safe attribute.
/// The function must accept a context followed by one owned argument type and return a
/// `Result<PreparedToolCall>`. The macro does not register tools or grant permissions.
#[proc_macro_attribute]
pub fn tool(attributes: TokenStream, item: TokenStream) -> TokenStream {
    let attributes = parse_macro_input!(attributes with Punctuated::<MetaNameValue, Token![,]>::parse_terminated);
    let function = parse_macro_input!(item as ItemFn);
    match expand_tool(attributes, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_tool(
    attributes: Punctuated<MetaNameValue, Token![,]>,
    function: ItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    if function.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            function.sig.fn_token,
            "tool prepare function must be async",
        ));
    }
    let mut adapter = None;
    let mut name = None;
    let mut description = None;
    let mut category = None;
    let mut risk = None;
    let mut requires = None;
    let mut parallel_safe = None;
    let mut group = None;
    let mut default_enabled = None;
    let mut platform = None;
    let mut runtime = None;
    let mut concurrency = None;
    let mut lifetime = None;
    for attribute in attributes {
        if attribute.path.is_ident("adapter") {
            set_once(
                &mut adapter,
                parse_ident(string_value(&attribute.value)?)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("name") {
            set_once(
                &mut name,
                string_value(&attribute.value)?.clone(),
                &attribute,
            )?;
        } else if attribute.path.is_ident("description") {
            set_once(
                &mut description,
                string_value(&attribute.value)?.clone(),
                &attribute,
            )?;
        } else if attribute.path.is_ident("category") {
            set_once(
                &mut category,
                parse_category(string_value(&attribute.value)?)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("risk") {
            set_once(
                &mut risk,
                parse_risk(string_value(&attribute.value)?)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("requires") {
            set_once(&mut requires, parse_requires(&attribute.value)?, &attribute)?;
        } else if attribute.path.is_ident("group") {
            set_once(
                &mut group,
                policy_value(
                    string_value(&attribute.value)?,
                    "ToolGroup",
                    &[("jadx", "Jadx"), ("tailcat", "Tailcat")],
                )?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("default_enabled") {
            set_once(
                &mut default_enabled,
                bool_value(&attribute.value)?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("platform") {
            set_once(
                &mut platform,
                policy_value(
                    string_value(&attribute.value)?,
                    "ToolPlatform",
                    &[
                        ("any", "Any"),
                        ("android", "Android"),
                        ("android_shell", "AndroidShell"),
                        ("android_or_linux", "AndroidOrLinux"),
                    ],
                )?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("runtime") {
            set_once(
                &mut runtime,
                policy_value(
                    string_value(&attribute.value)?,
                    "RuntimeRequirement",
                    &[("none", "None"), ("tailcat", "Tailcat"), ("jadx", "Jadx")],
                )?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("concurrency") {
            set_once(
                &mut concurrency,
                policy_value(
                    string_value(&attribute.value)?,
                    "ToolConcurrency",
                    &[
                        ("parallel", "Parallel"),
                        ("sequential", "Sequential"),
                        ("android_ui", "AndroidUi"),
                        ("shell", "Shell"),
                    ],
                )?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("lifetime") {
            set_once(
                &mut lifetime,
                policy_value(
                    string_value(&attribute.value)?,
                    "ToolLifetime",
                    &[("call", "Call"), ("process", "Process")],
                )?,
                &attribute,
            )?;
        } else if attribute.path.is_ident("parallel_safe") {
            set_once(
                &mut parallel_safe,
                bool_value(&attribute.value)?,
                &attribute,
            )?;
        } else {
            return Err(syn::Error::new_spanned(
                attribute.path,
                "unknown tool attribute",
            ));
        }
    }
    let adapter =
        adapter.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing adapter"))?;
    let name_value =
        name.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing name"))?;
    let description = description
        .ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing description"))?;
    let category =
        category.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing category"))?;
    let risk = risk.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing risk"))?;
    let requires =
        requires.ok_or_else(|| syn::Error::new_spanned(&function.sig.ident, "missing requires"))?;
    let concurrency = match (concurrency, parallel_safe) {
        (Some(policy), None) => policy,
        (None, Some(safe)) => {
            quote! { if #safe { crate::tools::ToolConcurrency::Parallel } else { crate::tools::ToolConcurrency::Sequential } }
        }
        _ => {
            return Err(syn::Error::new_spanned(
                &function.sig.ident,
                "provide exactly one of concurrency or parallel_safe",
            ))
        }
    };
    let default_enabled = default_enabled.unwrap_or(group.is_none());
    let group = group
        .map(|value| quote! { Some(#value) })
        .unwrap_or_else(|| quote! { None });
    let platform = platform.unwrap_or_else(|| quote! { crate::tools::ToolPlatform::Any });
    let runtime = runtime.unwrap_or_else(|| quote! { crate::tools::RuntimeRequirement::None });
    let lifetime = lifetime.unwrap_or_else(|| quote! { crate::tools::ToolLifetime::Call });
    let metadata = format_ident!("{}_METADATA", adapter.to_string().to_uppercase());
    if function.sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            &function.sig.inputs,
            "tool prepare function needs context and arguments",
        ));
    }
    let args = match function.sig.inputs.iter().nth(1) {
        Some(syn::FnArg::Typed(argument)) => &argument.ty,
        _ => {
            return Err(syn::Error::new_spanned(
                &function.sig.inputs,
                "second parameter must be typed arguments",
            ))
        }
    };
    let name = &function.sig.ident;
    Ok(quote! {
        #function

        const #metadata: crate::tools::ToolMetadata = crate::tools::ToolMetadata {
            name: #name_value,
            description: #description,
            category: #category,
            risk: #risk,
            requires: &[#(#requires),*],
            group: #group, default_enabled: #default_enabled,
            platform: #platform, runtime: #runtime, lifetime: #lifetime, concurrency: #concurrency,
            schema: crate::tools::descriptor_schema::<#args>,
        };

        pub(super) struct #adapter;

        #[async_trait::async_trait]
        impl crate::tools::Tool for #adapter {
            fn metadata(&self) -> &'static crate::tools::ToolMetadata {
                &#metadata
            }

            async fn prepare(
                &self,
                ctx: &crate::tools::ToolContext<'_>,
                arguments: serde_json::Value,
            ) -> anyhow::Result<crate::tools::PreparedToolCall> {
                let args: #args = crate::tools::parse_args(#metadata.name, arguments)?;
                #name(ctx, args).await
            }
        }
    })
}

fn policy_value(
    value: &LitStr,
    kind: &str,
    allowed: &[(&str, &str)],
) -> syn::Result<proc_macro2::TokenStream> {
    let selected = allowed
        .iter()
        .find(|(name, _)| *name == value.value())
        .ok_or_else(|| syn::Error::new_spanned(value, format!("unsupported {kind} policy")))?;
    let enumeration = format_ident!("{kind}");
    let variant = format_ident!("{}", selected.1);
    Ok(quote! { crate::tools::#enumeration::#variant })
}

fn set_once<T>(slot: &mut Option<T>, value: T, attribute: &MetaNameValue) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new_spanned(
            &attribute.path,
            "duplicate tool attribute",
        ));
    }
    *slot = Some(value);
    Ok(())
}

fn string_value(value: &Expr) -> syn::Result<&LitStr> {
    match value {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Ok(value),
        _ => Err(syn::Error::new_spanned(value, "expected a string literal")),
    }
}

fn bool_value(value: &Expr) -> syn::Result<bool> {
    match value {
        Expr::Lit(syn::ExprLit {
            lit: Lit::Bool(value),
            ..
        }) => Ok(value.value),
        _ => Err(syn::Error::new_spanned(value, "expected a bool literal")),
    }
}

fn parse_category(value: &LitStr) -> syn::Result<proc_macro2::TokenStream> {
    match value.value().as_str() {
        "shell" => Ok(quote!(crate::tools::ToolCategory::Shell)),
        "file" => Ok(quote!(crate::tools::ToolCategory::File)),
        "knowledge" => Ok(quote!(crate::tools::ToolCategory::Knowledge)),
        _ => Err(syn::Error::new_spanned(value, "unsupported tool category")),
    }
}

fn parse_risk(value: &LitStr) -> syn::Result<proc_macro2::TokenStream> {
    match value.value().as_str() {
        "dynamic_shell" => Ok(quote!(crate::tools::ToolRisk::DynamicShell)),
        "read_only" => Ok(quote!(crate::tools::ToolRisk::ReadOnly)),
        "mutating" => Ok(quote!(crate::tools::ToolRisk::Mutating)),
        "dangerous" => Ok(quote!(crate::tools::ToolRisk::Dangerous)),
        "critical" => Ok(quote!(crate::tools::ToolRisk::Critical)),
        _ => Err(syn::Error::new_spanned(value, "unsupported tool risk")),
    }
}

fn parse_requires(value: &Expr) -> syn::Result<Vec<proc_macro2::TokenStream>> {
    let Expr::Array(array) = value else {
        return Err(syn::Error::new_spanned(value, "requires must be an array"));
    };
    array
        .elems
        .iter()
        .map(|item| match string_value(item)?.value().as_str() {
            "ima" => Ok(quote!(crate::tools::Capability::Ima)),
            _ => Err(syn::Error::new_spanned(item, "unsupported tool capability")),
        })
        .collect()
}

fn parse_ident(value: &LitStr) -> syn::Result<syn::Ident> {
    syn::parse_str(&value.value())
        .map_err(|_| syn::Error::new_spanned(value, "expected a Rust identifier"))
}

#[cfg(test)]
mod tests {
    use super::expand_tool;
    use syn::{parse::Parser, parse_quote, punctuated::Punctuated, ItemFn, MetaNameValue, Token};

    #[test]
    fn duplicate_risk_is_a_compile_error() -> syn::Result<()> {
        let attributes = Punctuated::<MetaNameValue, Token![,]>::parse_terminated.parse_str(
            "adapter = \"ExampleTool\", name = \"example\", description = \"example\", category = \"file\", risk = \"mutating\", risk = \"read_only\", requires = [], parallel_safe = false",
        )?;
        let function: ItemFn = parse_quote! {
            async fn prepare(_: &ToolContext<'_>, _: ExampleArgs) -> Result<PreparedToolCall> {
                Ok(PreparedToolCall::empty())
            }
        };
        assert!(expand_tool(attributes, function).is_err());
        Ok(())
    }
}
